use async_trait::async_trait;

use crate::Result;
use crate::document::Request;
use crate::model::{Assignment, CanteenMenu, Course, CourseFile, Document, Event, FileEntry};

#[async_trait]
pub trait Courses: Send + Sync {
    async fn list_courses(&self) -> Result<Vec<Course>>;
    async fn list_course_files(&self, course_id: &str) -> Result<Vec<CourseFile>>;
    /// `None` lists the assignments of all enrolled courses.
    async fn list_assignments(&self, course_id: Option<&str>) -> Result<Vec<Assignment>>;
    async fn read_file(&self, file_ref: &str, request: Request<'_>) -> Result<Document>;
}

#[async_trait]
pub trait Calendar: Send + Sync {
    /// Events starting on the days `from` to `to` (`YYYY-MM-DD`, both included),
    /// earliest first. `from` defaults to today, `to` to six days after `from`.
    /// `search` keeps only events whose title contains it.
    async fn events(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        search: Option<&str>,
    ) -> Result<Vec<Event>>;
}

#[async_trait]
pub trait Canteens: Send + Sync {
    /// The menus of `date` (`YYYY-MM-DD`, today by default), for every
    /// canteen or only those whose name contains `canteen`.
    async fn menus(&self, date: Option<&str>, canteen: Option<&str>) -> Result<Vec<CanteenMenu>>;
}

#[async_trait]
pub trait Files: Send + Sync {
    async fn list(&self, path: &str) -> Result<Vec<FileEntry>>;
    async fn read(&self, path: &str, request: Request<'_>) -> Result<Document>;
}
