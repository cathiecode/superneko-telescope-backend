use url::Url;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Host {
  base_url: Url
}

impl Host {
  fn new(url: Url) -> Self {
    // FIXME: check url

    Self {
      base_url: url
    }
  }
  fn base_url(&self) -> &Url {
    &self.base_url
  }
}
