use async_trait::async_trait;
use serde::de::DeserializeOwned;
use uni_core::document::{self, View};
use uni_core::model::{Assignment, Course, CourseFile, Document};
use uni_core::{Courses, Error, Result};
use url::Url;

use crate::{SERVICE, wire};

const FILE_PATH_PREFIX: &str = "/webservice/pluginfile.php/";

pub struct MoodleClient {
    http: reqwest::Client,
    base_url: Url,
    token: String,
}

impl MoodleClient {
    pub fn new(http: reqwest::Client, base_url: &str, token: String) -> Result<Self> {
        let base_url = Url::parse(base_url)
            .map_err(|e| Error::Config(format!("invalid Moodle URL `{base_url}`: {e}")))?;
        Ok(Self {
            http,
            base_url,
            token,
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        function: &str,
        args: &[(String, String)],
    ) -> Result<T> {
        let endpoint = self
            .base_url
            .join("/webservice/rest/server.php")
            .map_err(|e| protocol(e.to_string()))?;
        let mut form: Vec<(String, String)> = vec![
            ("wstoken".into(), self.token.clone()),
            ("wsfunction".into(), function.into()),
            ("moodlewsrestformat".into(), "json".into()),
            // Without the text filters, names come back in every language at once.
            ("moodlewssettingfilter".into(), "true".into()),
        ];
        form.extend_from_slice(args);

        let response = self
            .http
            .post(endpoint)
            .form(&form)
            .send()
            .await
            .map_err(network)?;
        let body = response
            .error_for_status()
            .map_err(network)?
            .bytes()
            .await
            .map_err(network)?;
        decode(&body)
    }

    /// The token is appended to this URL, so it has to be on the Moodle host.
    fn checked_file_url(&self, file_ref: &str) -> Result<Url> {
        let url = Url::parse(file_ref).map_err(|_| {
            Error::InvalidInput("file_ref is not a URL from list_course_files".into())
        })?;
        let same_origin = url.origin() == self.base_url.origin();
        if !same_origin || !url.path().starts_with(FILE_PATH_PREFIX) {
            return Err(Error::InvalidInput(
                "file_ref does not point to a file on the configured Moodle".into(),
            ));
        }
        Ok(url)
    }
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    if let Ok(exception) = serde_json::from_slice::<wire::Exception>(body) {
        return Err(match exception.errorcode.as_str() {
            "invalidtoken" | "accessexception" => Error::Auth {
                service: SERVICE.into(),
            },
            code => protocol(format!("{code}: {}", exception.message.unwrap_or_default())),
        });
    }
    serde_json::from_slice(body).map_err(|e| protocol(e.to_string()))
}

fn network(e: reqwest::Error) -> Error {
    // File URLs carry the token, so the URL is dropped from the message.
    Error::Network {
        service: SERVICE.into(),
        message: e.without_url().to_string(),
    }
}

fn protocol(message: String) -> Error {
    Error::Protocol {
        service: SERVICE.into(),
        message,
    }
}

fn parse_course_id(course_id: &str) -> Result<u64> {
    course_id
        .parse()
        .map_err(|_| Error::InvalidInput(format!("`{course_id}` is not a Moodle course id")))
}

#[async_trait]
impl Courses for MoodleClient {
    async fn list_courses(&self) -> Result<Vec<Course>> {
        let site: wire::SiteInfo = self.call("core_webservice_get_site_info", &[]).await?;
        let courses: Vec<wire::Course> = self
            .call(
                "core_enrol_get_users_courses",
                &[("userid".into(), site.userid.to_string())],
            )
            .await?;
        Ok(courses
            .into_iter()
            .map(|c| Course {
                id: c.id.to_string(),
                name: c.fullname,
                short_name: c.shortname,
            })
            .collect())
    }

    async fn list_course_files(&self, course_id: &str) -> Result<Vec<CourseFile>> {
        let id = parse_course_id(course_id)?;
        let sections: Vec<wire::Section> = self
            .call(
                "core_course_get_contents",
                &[("courseid".into(), id.to_string())],
            )
            .await?;
        Ok(course_files(sections))
    }

    async fn list_assignments(&self, course_id: Option<&str>) -> Result<Vec<Assignment>> {
        let args = match course_id {
            Some(id) => vec![("courseids[0]".to_owned(), parse_course_id(id)?.to_string())],
            None => Vec::new(),
        };
        let result: wire::Assignments = self.call("mod_assign_get_assignments", &args).await?;
        Ok(assignments(result))
    }

    async fn read_file(&self, file_ref: &str, pages: Option<&str>, view: View) -> Result<Document> {
        let mut url = self.checked_file_url(file_ref)?;
        url.query_pairs_mut().append_pair("token", &self.token);

        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(network)?
            .error_for_status()
            .map_err(network)?;
        document::check_size(response.content_length().unwrap_or(0))?;
        let bytes = response.bytes().await.map_err(network)?;
        document::read(&bytes, pages, view)
    }
}

fn course_files(sections: Vec<wire::Section>) -> Vec<CourseFile> {
    let mut files = Vec::new();
    for section in sections {
        for content in section.modules.into_iter().flat_map(|m| m.contents) {
            let (true, Some(file_ref)) = (content.kind == "file", content.fileurl) else {
                continue;
            };
            files.push(CourseFile {
                section: section.name.clone(),
                name: content.filename,
                size: content.filesize,
                mime_type: content.mimetype,
                file_ref,
            });
        }
    }
    files
}

fn assignments(result: wire::Assignments) -> Vec<Assignment> {
    let mut all = Vec::new();
    for course in result.courses {
        for a in course.assignments {
            all.push(Assignment {
                id: a.id.to_string(),
                course_id: course.id.to_string(),
                name: a.name,
                // Moodle uses 0 for "no deadline".
                due_at: (a.duedate > 0).then_some(a.duedate),
            });
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> MoodleClient {
        MoodleClient::new(
            reqwest::Client::new(),
            "https://moodle.example",
            "t0ken".into(),
        )
        .expect("client")
    }

    #[test]
    fn token_is_never_sent_to_another_host() {
        let c = client();
        assert!(
            c.checked_file_url("https://moodle.example/webservice/pluginfile.php/1/a.pdf")
                .is_ok()
        );
        assert!(
            c.checked_file_url("https://evil.example/webservice/pluginfile.php/1/a.pdf")
                .is_err()
        );
        assert!(
            c.checked_file_url("http://moodle.example/webservice/pluginfile.php/1/a.pdf")
                .is_err()
        );
        assert!(
            c.checked_file_url("https://moodle.example/login/index.php")
                .is_err()
        );
    }

    #[test]
    fn invalid_token_becomes_an_auth_error() {
        let body = br#"{"exception":"moodle_exception","errorcode":"invalidtoken","message":"x"}"#;
        assert!(matches!(
            decode::<wire::SiteInfo>(body),
            Err(Error::Auth { .. })
        ));
    }

    #[test]
    fn course_contents_are_flattened_to_files() {
        let body = br#"[{"name":"Week 1","modules":[
            {"contents":[{"type":"file","filename":"slides.pdf","filesize":10,
              "fileurl":"https://moodle.example/webservice/pluginfile.php/1/slides.pdf","mimetype":"application/pdf"},
             {"type":"url","filename":"link"}]},
            {}]}]"#;
        let files = course_files(decode(body).expect("sections"));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].section, "Week 1");
        assert_eq!(files[0].name, "slides.pdf");
    }

    #[test]
    fn zero_due_date_means_no_deadline() {
        let body = br#"{"courses":[{"id":7,"assignments":[
            {"id":1,"name":"Sheet 1","duedate":1760000000},{"id":2,"name":"Bonus","duedate":0}]}]}"#;
        let list = assignments(decode(body).expect("assignments"));
        assert_eq!(list[0].due_at, Some(1_760_000_000));
        assert_eq!(list[1].due_at, None);
        assert_eq!(list[1].course_id, "7");
    }
}
