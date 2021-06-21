#![allow(clippy::from_over_into)]
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{stderr, stdout, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::from_utf8;

use async_trait::async_trait;
use chrono::offset::Utc;
use chrono::DateTime;
use google_drive::GoogleDrive;
use hubcaps::repositories::Repository;
use hubcaps::Github;
use macros::db;
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::airtable::AIRTABLE_RFD_TABLE;
use crate::companies::Company;
use crate::core::UpdateAirtableRecord;
use crate::rfds::{clean_rfd_html_links, get_images_in_branch, get_rfd_contents_from_repo, parse_markdown, update_discussion_link, update_state};
use crate::schema::{rfds as r_f_ds, rfds};
use crate::utils::{create_or_update_file_in_github_repo, write_file};

/// The data type for an RFD.
#[db {
    new_struct_name = "RFD",
    airtable_base = "roadmap",
    airtable_table = "AIRTABLE_RFD_TABLE",
    match_on = {
        "number" = "i32",
    }
}]
#[derive(Debug, Insertable, AsChangeset, PartialEq, Clone, JsonSchema, Deserialize, Serialize)]
#[table_name = "rfds"]
pub struct NewRFD {
    // TODO: remove this alias when we update https://github.com/oxidecomputer/rfd/blob/master/.helpers/rfd.csv
    // When you do this you need to update src/components/images.js in the rfd repo as well.
    // those are the only two things remaining that parse the CSV directly.
    #[serde(alias = "num")]
    pub number: i32,
    /// (generated) number_string is the long version of the number with leading zeros
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub number_string: String,
    pub title: String,
    /// (generated) name is a combination of number and title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub state: String,
    /// link is the canonical link to the source.
    pub link: String,
    /// (generated) short_link is the generated link in the form of https://{number}.rfd.oxide.computer
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub short_link: String,
    /// (generated) rendered_link is the link to the rfd in the rendered html website in the form of
    /// https://rfd.shared.oxide.computer/rfd/{{number_string}}
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rendered_link: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub discussion: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub authors: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    /// sha is the SHA of the last commit that modified the file
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha: String,
    /// commit_date is the date of the last commit that modified the file
    #[serde(default = "Utc::now")]
    pub commit_date: DateTime<Utc>,
    /// milestones only exist in Airtable
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub milestones: Vec<String>,
    /// relevant_components only exist in Airtable
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relevant_components: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pdf_link_github: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pdf_link_google_drive: String,
    /// The CIO company ID.
    #[serde(default)]
    pub cio_company_id: i32,
}

impl NewRFD {
    /// Return a NewRFD from a parsed file on a specific GitHub branch.
    pub async fn new_from_github(company: &Company, repo: &Repository, branch: &str, file_path: &str, commit_date: DateTime<Utc>) -> Self {
        // Get the file from GitHub.
        let mut content = String::new();
        let mut link = String::new();
        let mut sha = String::new();
        if let Ok(file) = repo.content().file(file_path, branch).await {
            content = from_utf8(&file.content).unwrap().trim().to_string();
            link = file.html_url;
            sha = file.sha;
        }

        // Parse the RFD directory as an int.
        let (dir, _) = file_path.trim_start_matches("rfd/").split_once('/').unwrap();
        let number = dir.trim_start_matches('0').parse::<i32>().unwrap();

        let number_string = NewRFD::generate_number_string(number);

        // Parse the RFD title from the contents.
        let title = NewRFD::get_title(&content);
        let name = NewRFD::generate_name(number, &title);

        // Parse the state from the contents.
        let state = NewRFD::get_state(&content);

        // Parse the discussion from the contents.
        let discussion = NewRFD::get_discussion(&content);

        NewRFD {
            number,
            number_string,
            title,
            name,
            state,
            link,
            short_link: Default::default(),
            rendered_link: Default::default(),
            discussion,
            authors: Default::default(),
            // We parse this below.
            html: Default::default(),
            content,
            sha,
            commit_date,
            // Only exists in Airtable,
            milestones: Default::default(),
            // Only exists in Airtable,
            relevant_components: Default::default(),
            pdf_link_github: Default::default(),
            pdf_link_google_drive: Default::default(),
            cio_company_id: company.id,
        }
    }

    pub fn get_title(content: &str) -> String {
        let mut re = Regex::new(r"(?m)(RFD .*$)").unwrap();
        match re.find(&content) {
            Some(v) => {
                // TODO: find less horrible way to do this.
                let trimmed = v.as_str().replace("RFD", "").replace("# ", "").replace("= ", " ").trim().to_string();

                let (_, s) = trimmed.split_once(' ').unwrap();

                // If the string is empty, it means there is no RFD in our
                // title.
                if s.is_empty() {}

                s.to_string()
            }
            None => {
                // There is no "RFD" in our title. This is the case for RFD 31.
                re = Regex::new(r"(?m)(^= .*$)").unwrap();
                let results = re.find(&content).unwrap();
                results.as_str().replace("RFD", "").replace("# ", "").replace("= ", " ").trim().to_string()
            }
        }
    }

    pub fn get_state(content: &str) -> String {
        let re = Regex::new(r"(?m)(state:.*$)").unwrap();
        match re.find(&content) {
            Some(v) => return v.as_str().replace("state:", "").trim().to_string(),
            None => Default::default(),
        }
    }

    pub fn get_discussion(content: &str) -> String {
        let re = Regex::new(r"(?m)(discussion:.*$)").unwrap();
        match re.find(&content) {
            Some(v) => return v.as_str().replace("discussion:", "").trim().to_string(),
            None => Default::default(),
        }
    }

    pub fn generate_number_string(number: i32) -> String {
        // Add leading zeros to the number for the number_string.
        let mut number_string = number.to_string();
        while number_string.len() < 4 {
            number_string = format!("0{}", number_string);
        }

        number_string
    }

    pub fn generate_name(number: i32, title: &str) -> String {
        format!("RFD {} {}", number, title)
    }

    pub fn generate_short_link(number: i32) -> String {
        format!("https://{}.rfd.oxide.computer", number)
    }

    pub fn generate_rendered_link(number_string: &str) -> String {
        format!("https://rfd.shared.oxide.computer/rfd/{}", number_string)
    }

    pub fn get_authors(content: &str, is_markdown: bool) -> String {
        if is_markdown {
            // TODO: make work w asciidoc.
            let re = Regex::new(r"(?m)(^authors.*$)").unwrap();
            match re.find(&content) {
                Some(v) => return v.as_str().replace("authors:", "").trim().to_string(),
                None => Default::default(),
            }
        }

        // We must have asciidoc content.
        // We want to find the line under the first "=" line (which is the title), authors is under
        // that.
        let re = Regex::new(r"(?m:^=.*$)[\n\r](?m)(.*$)").unwrap();
        match re.find(&content) {
            Some(v) => {
                let val = v.as_str().trim().to_string();
                let parts: Vec<&str> = val.split('\n').collect();
                if parts.len() < 2 {
                    Default::default()
                } else {
                    let mut authors = parts[1].to_string();
                    if authors == "{authors}" {
                        // Do the traditional check.
                        let re = Regex::new(r"(?m)(^:authors.*$)").unwrap();
                        if let Some(v) = re.find(&content) {
                            authors = v.as_str().replace(":authors:", "").trim().to_string();
                        }
                    }
                    authors
                }
            }
            None => Default::default(),
        }
    }
}

impl RFD {
    pub async fn get_html(&self, repo: &Repository, branch: &str, is_markdown: bool) -> String {
        let html: String;
        if is_markdown {
            // Parse the markdown.
            html = parse_markdown(&self.content);
        } else {
            // Parse the acsiidoc.
            html = self.parse_asciidoc(repo, branch).await;
        }

        clean_rfd_html_links(&html, &self.number_string)
    }

    pub async fn parse_asciidoc(&self, repo: &Repository, branch: &str) -> String {
        let dir = format!("rfd/{}", self.number_string);

        // Create the temporary directory.
        let mut path = env::temp_dir();
        path.push("asciidoc-temp/");
        let pparent = path.clone();
        let parent = pparent.as_path().to_str().unwrap().trim_end_matches('/');
        path.push("contents.adoc");

        // Write the contents to a temporary file.
        write_file(&path, &deunicode::deunicode(&self.content));

        // If the file contains inline images, we need to save those images locally.
        // TODO: we don't need to save all the images, only the inline ones, clean this up
        // eventually.
        if self.content.contains("[opts=inline]") {
            let images = get_images_in_branch(repo, &dir, branch).await;
            for image in images {
                // Save the image to our temporary directory.
                let image_path = format!("{}/{}", parent, image.path.replace(&dir, "").trim_start_matches('/'));

                write_file(&PathBuf::from(image_path), from_utf8(&image.content).unwrap_or_default());
            }
        }

        let cmd_output = Command::new("asciidoctor")
            .current_dir(parent)
            .args(&["-o", "-", "--no-header-footer", path.to_str().unwrap()])
            .output()
            .unwrap();

        let result = if cmd_output.status.success() {
            from_utf8(&cmd_output.stdout).unwrap()
        } else {
            println!("[rfds] running asciidoctor failed:");
            stdout().write_all(&cmd_output.stdout).unwrap();
            stderr().write_all(&cmd_output.stderr).unwrap();

            Default::default()
        };

        // Delete the parent directory.
        let pdir = Path::new(parent);
        if pdir.exists() && pdir.is_dir() {
            fs::remove_dir_all(pdir).unwrap();
        }

        result.to_string()
    }

    /// Convert an RFD into JSON as Slack message.
    // TODO: make this include more fields
    pub fn as_slack_msg(&self) -> String {
        let mut msg = format!("{} (_*{}*_) <{}|github> <{}|rendered>", self.name, self.state, self.short_link, self.rendered_link);

        if !self.discussion.is_empty() {
            msg += &format!(" <{}|discussion>", self.discussion);
        }

        msg
    }

    /// Get a changelog for the RFD.
    pub async fn get_weekly_changelog(&self, github: &Github, since: DateTime<Utc>, company: &Company) -> String {
        let repo = github.repo(&company.github_org, "rfd");
        let r = repo.get().await.unwrap();
        let mut changelog = String::new();

        let mut branch = self.number_string.to_string();
        if self.link.contains(&format!("/{}/", r.default_branch)) {
            branch = r.default_branch.to_string();
        }

        // Get the commits from the last seven days to the file.
        let commits = repo.commits().list(&format!("/rfd/{}/", self.number_string), &branch, Some(since)).await.unwrap();

        for commit in commits {
            let message: Vec<&str> = commit.commit.message.lines().collect();
            if !message.is_empty() {
                changelog += &format!("\t- \"{}\" by @{}\n\t\thttps://github.com/oxidecomputer/rfd/commit/{}\n", message[0], commit.author.login, commit.sha);
            }
        }

        changelog
    }

    /// Get the filename for the PDF of the RFD.
    pub fn get_pdf_filename(&self) -> String {
        format!("RFD {} {}.pdf", self.number_string, self.title.replace("/", "-").replace("'", "").replace(":", "").trim())
    }

    /// Update an RFDs state.
    pub fn update_state(&mut self, state: &str, is_markdown: bool) {
        self.content = update_state(&self.content, state, is_markdown);
        self.state = state.to_string();
    }

    /// Update an RFDs discussion link.
    pub fn update_discussion(&mut self, link: &str, is_markdown: bool) {
        self.content = update_discussion_link(&self.content, link, is_markdown);
        self.discussion = link.to_string();
    }

    /// Convert the RFD content to a PDF and upload the PDF to the /pdfs folder of the RFD
    /// repository.
    pub async fn convert_and_upload_pdf(&mut self, github: &Github, drive_client: &GoogleDrive, company: &Company) {
        // Get the rfd repo client.
        let rfd_repo = github.repo(&company.github_org, "rfd");
        let repo = rfd_repo.get().await.unwrap();

        let mut path = env::temp_dir();
        path.push(format!("pdfcontents{}.adoc", self.number_string));

        let rfd_content = self.content.to_string();

        // Write the contents to a temporary file.
        let mut file = fs::File::create(path.clone()).unwrap();
        file.write_all(rfd_content.as_bytes()).unwrap();

        let file_name = self.get_pdf_filename();
        let rfd_path = format!("/pdfs/{}", file_name);

        let mut branch = self.number_string.to_string();
        if self.link.contains(&format!("/{}/", repo.default_branch)) {
            branch = repo.default_branch.to_string();
        }

        // Create the dir where to save images.
        let temp_dir = env::temp_dir();
        let temp_dir_str = temp_dir.to_str().unwrap();

        // We need to save the images locally as well.
        // This ensures that
        let old_dir = format!("rfd/{}", self.number_string);
        let images = get_images_in_branch(&rfd_repo, &old_dir, &branch).await;
        for image in images {
            // Save the image to our temporary directory.
            let image_path = format!("{}/{}", temp_dir_str.trim_end_matches('/'), image.path.replace(&old_dir, "").trim_start_matches('/'));

            write_file(&PathBuf::from(image_path), from_utf8(&image.content).unwrap_or_default());
        }

        let cmd_output = Command::new("asciidoctor-pdf")
            .current_dir(env::temp_dir())
            .args(&["-o", "-", "-a", "source-highlighter=rouge", path.to_str().unwrap()])
            .output()
            .unwrap();

        if !cmd_output.status.success() {
            println!("[rfdpdf] running asciidoctor failed:");
            stdout().write_all(&cmd_output.stdout).unwrap();
            stderr().write_all(&cmd_output.stderr).unwrap();
            return;
        }

        // Create or update the file in the github repository.
        create_or_update_file_in_github_repo(&rfd_repo, &repo.default_branch, &rfd_path, cmd_output.stdout.clone()).await;

        // Figure out where our directory is.
        // It should be in the shared drive : "Automated Documents"/"rfds"
        let shared_drive = drive_client.get_drive_by_name("Automated Documents").await.unwrap();
        let drive_id = shared_drive.id.to_string();

        // Get the directory by the name.
        let drive_rfd_dir = drive_client.get_file_by_name(&drive_id, "rfds").await.unwrap();
        let parent_id = drive_rfd_dir.get(0).unwrap().id.to_string();

        // Create or update the file in the google_drive.
        let drive_file = drive_client
            .create_or_update_file(&drive_id, &parent_id, &file_name, "application/pdf", &cmd_output.stdout)
            .await
            .unwrap();
        self.pdf_link_google_drive = format!("https://drive.google.com/open?id={}", drive_file.id);

        // Delete our temporary file.
        if path.exists() && !path.is_dir() {
            fs::remove_file(path).unwrap();
        }
    }

    /// Expand the fields in the RFD.
    /// This will get the content, html, sha, commit_date as well as fill in all generated fields.
    pub async fn expand(&mut self, github: &Github, company: &Company) {
        let repo = github.repo(&company.github_org, "rfd");
        let r = repo.get().await.unwrap();

        // Trim the title.
        self.title = self.title.trim().to_string();

        // Add leading zeros to the number for the number_string.
        self.number_string = NewRFD::generate_number_string(self.number);

        // Set the full name.
        self.name = NewRFD::generate_name(self.number, &self.title);

        // Set the short_link.
        self.short_link = NewRFD::generate_short_link(self.number);
        // Set the rendered_link.
        self.rendered_link = NewRFD::generate_rendered_link(&self.number_string);

        let mut branch = self.number_string.to_string();
        if self.link.contains(&format!("/{}/", r.default_branch)) {
            branch = r.default_branch.to_string();
        }

        // Get the RFD contents from the branch.
        let rfd_dir = format!("/rfd/{}", self.number_string);
        let (rfd_content, is_markdown, sha) = get_rfd_contents_from_repo(github, &branch, &rfd_dir, company).await;
        self.content = rfd_content;
        self.sha = sha;

        if branch == r.default_branch {
            // Get the commit date.
            if let Ok(commits) = repo.commits().list(&rfd_dir, "", None).await {
                let commit = commits.get(0).unwrap();
                self.commit_date = commit.commit.author.date;
            }
        } else {
            // Get the branch.
            if let Ok(commit) = repo.commits().get(&branch).await {
                // TODO: we should not have to duplicate this code below
                // but the references were mad...
                self.commit_date = commit.commit.author.date;
            }
        }

        // Parse the HTML.
        self.html = self.get_html(&repo, &branch, is_markdown).await;

        self.authors = NewRFD::get_authors(&self.content, is_markdown);

        // Set the pdf link
        let file_name = self.get_pdf_filename();
        let rfd_path = format!("/pdfs/{}", file_name);
        self.pdf_link_github = format!("https://github.com/{}/rfd/blob/master{}", company.github_org, rfd_path);

        self.cio_company_id = company.id;
    }
}

/// Implement updating the Airtable record for an RFD.
#[async_trait]
impl UpdateAirtableRecord<RFD> for RFD {
    async fn update_airtable_record(&mut self, record: RFD) {
        // Set the Link to People from the original so it stays intact.
        self.milestones = record.milestones.clone();
        self.relevant_components = record.relevant_components;
        // Airtable can only hold 100,000 chars. IDK which one is that long but LOL
        // https://community.airtable.com/t/what-is-the-long-text-character-limit/1780
        self.content = truncate(&self.content, 100000);
        self.html = truncate(&self.html, 100000);
    }
}

pub fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        None => s.to_string(),
        Some((idx, _)) => s[..idx].to_string(),
    }
}

pub fn get_value(map: &HashMap<String, Vec<String>>, key: &str) -> String {
    let empty: Vec<String> = Default::default();
    let a = map.get(key).unwrap_or(&empty);

    if a.is_empty() {
        return Default::default();
    }

    a.get(0).unwrap().to_string()
}
