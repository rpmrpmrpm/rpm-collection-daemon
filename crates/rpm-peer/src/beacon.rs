//! The multicast DNS announcement, so rpm finds the NAS and its libraries without an address.

use std::collections::HashMap;

use mdns_sd::{ServiceDaemon, ServiceInfo};
use rpm_wire::txt::SERVICE_TYPE;

/// The longest DNS label. An instance name is one label.
const MAX_LABEL: usize = 63;

/// One responder for the whole process. Each announcement registers a service on it.
pub struct Announcer {
    daemon: ServiceDaemon,
    host: String,
}

impl Announcer {
    /// A responder that answers for `host_label`.local, with the machine's addresses on every
    /// interface.
    pub fn new(host_label: &str) -> Result<Announcer, mdns_sd::Error> {
        Ok(Announcer {
            daemon: ServiceDaemon::new()?,
            host: format!("{}.local.", host_name(host_label)),
        })
    }

    /// Announces `instance` on `port` with the TXT record `txt`, until the returned value is dropped.
    pub fn announce(
        &self,
        instance: &str,
        port: u16,
        txt: &[(&str, String)],
    ) -> Result<Announcement, mdns_sd::Error> {
        let service_type = format!("{SERVICE_TYPE}.local.");
        let properties: HashMap<String, String> = txt
            .iter()
            .map(|(key, value)| (key.to_string(), value.clone()))
            .collect();
        let info = ServiceInfo::new(
            &service_type,
            &label(instance),
            &self.host,
            "",
            port,
            properties,
        )?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        self.daemon.register(info)?;
        Ok(Announcement {
            daemon: self.daemon.clone(),
            fullname,
        })
    }
}

/// A registered service. Dropping it withdraws the announcement.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Drop for Announcement {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
    }
}

/// `text` cut to one DNS label, at a character boundary.
pub fn label(text: &str) -> String {
    let mut end = text.len().min(MAX_LABEL);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].trim_end_matches('.').to_string()
}

/// A host name made only of letters, digits and dashes.
fn host_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() {
        "rpm-nas".to_string()
    } else {
        label(cleaned)
    }
}
