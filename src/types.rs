use url::Url;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Host {
  base_url: Url
}

impl Host {
  pub fn new(url: Url) -> Self {
    // FIXME: check url

    Self {
      base_url: url
    }
  }
  pub fn base_url(&self) -> &Url {
    &self.base_url
  }
}
