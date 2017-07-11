/*
 * Copyright (c) 2017 Pascal Bach
 *
 * SPDX-License-Identifier:     MIT
 */

// Get Max of u32
use std::u32;

// Used for error and debug logging
extern crate log;

// Used for gitlab API access via HTTPS
#[cfg(feature = "native-tls")]
extern crate hyper_native_tls;
#[cfg(not(feature = "native-tls"))]
extern crate hyper_rustls;
use hyper::client::Client;
use hyper::header::Headers;
use hyper::status::StatusCode;
use hyper::net::HttpsConnector;

// Custom header used to access the gitlab API
// See: https://docs.gitlab.com/ce/api/#authentication
header! { (PrivateToken, "PRIVATE-TOKEN") => [String] }
header! { (XNextPage, "X-Next-Page") => [u32] }

// Used to serialize JSON and YAML responses from the API
extern crate serde;
extern crate serde_json;
extern crate serde_yaml;

use provider::{Mirror, Provider};

#[derive(Debug)]
pub struct GitLab {
    pub url: String,
    pub group: String,
    pub use_http: bool,
    pub private_token: Option<String>,
}

/// A structured description
#[derive(Deserialize, Debug)]
struct Desc {
    origin: String,
    #[serde(default)]
    skip: bool,
}

/// A project from the GitLab API
#[derive(Deserialize, Debug, Clone)]
struct Project {
    description: String,
    web_url: String,
    ssh_url_to_repo: String,
    http_url_to_repo: String,
}

const PER_PAGE: u8 = 100;

impl Provider for GitLab {
    fn get_mirror_repos(&self) -> Result<Vec<Mirror>, String> {

        #[cfg(feature = "native-tls")]
        let tls =
            hyper_native_tls::NativeTlsClient::new().expect("Unable to initialize TLS system");
        #[cfg(not(feature = "native-tls"))]
        let tls = hyper_rustls::TlsClient::new();

        let connector = HttpsConnector::new(tls);
        let client = Client::with_connector(connector);

        let use_http = self.use_http;

        let mut headers = Headers::new();
        match self.private_token.clone() {
            Some(token) => {
                headers.set(PrivateToken(token));
            }
            None => trace!("GITLAB_PRIVATE_TOKEN not set"),
        }

        let mut projects: Vec<Project> = Vec::new();

        for page in 1..u32::MAX {

            let url = format!(
                "{}/api/v4/groups/{}/projects?per_page={}&page={}",
                self.url,
                self.group,
                PER_PAGE,
                page
            );
            trace!("URL: {}", url);

            let res = client.get(&url).headers(headers.clone()).send().or_else(
                |e| {
                    Err(format!("Unable to connect to: {} ({})", url, e))
                },
            )?;

            if res.status != StatusCode::Ok {
                if res.status == StatusCode::Unauthorized {
                    return Err(format!(
                        "API call received unautorized ({}) for: {}. \
                                   Please make sure the `GITLAB_PRIVATE_TOKEN` environment \
                                   variable is set.",
                        res.status,
                        url
                    ));
                } else {
                    return Err(format!(
                        "API call received invalid status ({}) for : {}",
                        res.status,
                        url
                    ));
                }
            }


            let has_next = match res.headers.get::<XNextPage>() {
                None => {
                    trace!("No more pages");
                    false
                },
                Some(n) => {
                    trace!("Next page: {}", n);
                    true
                }
            };

            let projects_page: Vec<Project> = serde_json::from_reader(res).or_else(|e| {
                Err(format!("Unable to parse response as JSON ({})", e))
            })?;

            projects.extend(projects_page);

            if !has_next {
                break;
            }
        }

        let mut mirrors: Vec<Mirror> = Vec::new();

        for p in projects {
            match serde_yaml::from_str::<Desc>(&p.description) {
                Ok(desc) => {
                    if desc.skip {
                        warn!("Skipping {}, Skip flag set", p.web_url);
                        continue;
                    }
                    trace!("{0} -> {1}", desc.origin, p.ssh_url_to_repo);
                    let destination = if use_http {
                        p.http_url_to_repo
                    } else {
                        p.ssh_url_to_repo
                    };
                    let m = Mirror {
                        origin: desc.origin,
                        destination: destination,
                    };
                    mirrors.push(m);
                }
                Err(e) => warn!("Skipping {}, Description not valid YAML ({})", p.web_url, e),
            }
        }

        return Ok(mirrors);
    }
}
