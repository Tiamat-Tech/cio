use handlebars::{Context, Handlebars, Helper, HelperResult, Output, RenderContext};
use hubcaps::repositories::Repository;
use tracing::instrument;

use crate::shorturls::ShortUrl;
use crate::utils::create_or_update_file_in_github_repo;

/// Helper function so the terraform names do not start with a number.
/// Otherwise terraform will fail.
fn terraform_name_helper(h: &Helper, _: &Handlebars, _: &Context, _rc: &mut RenderContext, out: &mut dyn Output) -> HelperResult {
    let p = h.param(0).unwrap().value().to_string();
    let param = p.trim_matches('"');

    // Check if the first character is a number.
    let first_char = param.chars().next().unwrap();
    if first_char.is_digit(10) {
        out.write(&("_".to_owned() + param))?;
    } else {
        out.write(&param)?;
    }
    Ok(())
}

/// Generate nginx and terraform files for shorturls.
/// This is used for short URL link generation like:
///   - {link}.corp.oxide.computer
///   - {repo}.git.oxide.computer
///   - {num}.rfd.oxide.computer
/// This function saves the generated files in the GitHub repository, in the
/// given path.
#[instrument(skip(repo))]
#[inline]
pub async fn generate_nginx_and_terraform_files_for_shorturls(repo: &Repository, shorturls: Vec<ShortUrl>) {
    if shorturls.is_empty() {
        println!("no shorturls in array");
        return;
    }

    // Initialize handlebars.
    let mut handlebars = Handlebars::new();
    handlebars.register_helper("terraformize", Box::new(terraform_name_helper));

    // Get the subdomain from the first link.
    let subdomain = shorturls[0].subdomain.to_string();

    // Generate the subdomains nginx file.
    let nginx_file = format!("/nginx/conf.d/generated.{}.oxide.computer.conf", subdomain);
    // Add a warning to the top of the file that it should _never_
    // be edited by hand and generate it.
    let mut nginx_rendered = TEMPLATE_WARNING.to_owned() + &handlebars.render_template(&TEMPLATE_NGINX, &shorturls).unwrap();
    // Add the vim formating string.
    nginx_rendered += "# vi: ft=nginx";

    // TODO: actually get the main branch from the GitHub API in case it changes in the future.
    create_or_update_file_in_github_repo(repo, "master", &nginx_file, nginx_rendered.as_bytes().to_vec()).await;

    // Generate the paths nginx file.
    let nginx_paths_file = format!("/nginx/conf.d/generated.{}.paths.oxide.computer.conf", subdomain);
    // Add a warning to the top of the file that it should _never_
    // be edited by hand and generate it.
    let mut nginx_paths_rendered = TEMPLATE_WARNING.to_owned() + &handlebars.render_template(&TEMPLATE_NGINX_PATHS, &shorturls).unwrap();
    // Add the vim formating string.
    nginx_paths_rendered += "# vi: ft=nginx";

    // TODO: actually get the main branch from the GitHub API in case it changes in the future.
    create_or_update_file_in_github_repo(repo, "master", &nginx_paths_file, nginx_paths_rendered.as_bytes().to_vec()).await;

    // Generate the terraform file.
    let terraform_file = format!("/terraform/cloudflare/generated.{}.oxide.computer.tf", subdomain);
    // Add a warning to the top of the file that it should _never_
    // be edited by hand and generate it.
    let terraform_rendered = TEMPLATE_WARNING.to_owned() + &handlebars.render_template(&TEMPLATE_CLOUDFLARE_TERRAFORM, &shorturls).unwrap();

    // TODO: actually get the main branch from the GitHub API in case it changes in the future.
    create_or_update_file_in_github_repo(repo, "master", &terraform_file, terraform_rendered.as_bytes().to_vec()).await;
}

/// The warning for files that we automatically generate so folks don't edit them
/// all willy nilly.
pub static TEMPLATE_WARNING: &str = "# THIS FILE HAS BEEN GENERATED BY THE CIO REPO
# AND SHOULD NEVER BE EDITED BY HAND!!
# Instead change the link in configs/links.toml
";

/// Template for creating nginx conf files for the subdomain urls.
pub static TEMPLATE_NGINX: &str = r#"{{#each this}}
# Redirect {{this.link}} to {{this.name}}.{{this.subdomain}}.oxide.computer
# Description: {{this.description}}
server {
	listen      [::]:443 ssl http2;
	listen      443 ssl http2;
	server_name {{this.name}}.{{this.subdomain}}.oxide.computer;

	include ssl-params.conf;

	ssl_certificate			/etc/nginx/ssl/wildcard.{{this.subdomain}}.oxide.computer/fullchain.pem;
	ssl_certificate_key		/etc/nginx/ssl/wildcard.{{this.subdomain}}.oxide.computer/privkey.pem;
	ssl_trusted_certificate	    	/etc/nginx/ssl/wildcard.{{this.subdomain}}.oxide.computer/fullchain.pem;

	# Add redirect.
	location / {
		return 301 "{{this.link}}";
	}

	{{#if this.discussion}}# Redirect /discussion to {{this.discussion}}
	# Description: Discussion link for {{this.description}}
	location /discussion {
		return 301 {{this.discussion}};
	}
{{/if}}
}
{{/each}}
"#;

/// Template for creating nginx conf files for the paths urls.
pub static TEMPLATE_NGINX_PATHS: &str = r#"server {
	listen      [::]:443 ssl http2;
	listen      443 ssl http2;
	server_name {{this.0.subdomain}}.oxide.computer;

	include ssl-params.conf;

	# Note this certificate is NOT the wildcard, since these are paths.
	ssl_certificate			/etc/nginx/ssl/{{this.0.subdomain}}.oxide.computer/fullchain.pem;
	ssl_certificate_key		/etc/nginx/ssl/{{this.0.subdomain}}.oxide.computer/privkey.pem;
	ssl_trusted_certificate	        /etc/nginx/ssl/{{this.0.subdomain}}.oxide.computer/fullchain.pem;

	location = / {
		return 301 https://github.com/oxidecomputer/meta/tree/master/links;
	}

	{{#each this}}
	# Redirect {{this.subdomain}}.oxide.computer/{{this.name}} to {{this.link}}
	# Description: {{this.description}}
	location = /{{this.name}} {
		return 301 "{{this.link}}";
	}
{{#if this.discussion}}	# Redirect /{{this.name}}/discussion to {{this.discussion}}
	# Description: Discussion link for {{this.name}}
	location = /{{this.name}}/discussion {
		return 301 {{this.discussion}};
	}
{{/if}}
{{/each}}
}
"#;

/// Template for creating DNS records in our Cloudflare terraform configs.
pub static TEMPLATE_CLOUDFLARE_TERRAFORM: &str = r#"{{#each this}}
resource "cloudflare_record" "{{terraformize this.name}}_{{this.subdomain}}_oxide_computer" {
  zone_id  = var.zone_id-oxide_computer
  name     = "{{this.name}}.{{this.subdomain}}.oxide.computer"
  value    = var.maverick_ip
  type     = "A"
  ttl      = 1
  priority = 0
  proxied  = false
}
{{/each}}
"#;
