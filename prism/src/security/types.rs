#[derive(Debug, Clone)]
pub struct AuthUser {
    pub name: String,
    pub roles: Vec<String>,
    pub key_prefix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Read,
    Write,
    Delete,
    Search,
    Admin,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Read => "read",
            Permission::Write => "write",
            Permission::Delete => "delete",
            Permission::Search => "search",
            Permission::Admin => "admin",
        }
    }
}
