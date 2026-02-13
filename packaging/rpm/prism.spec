Name:           prismsearch
Version:        0.6.2
Release:        1%{?dist}
Summary:        Hybrid search engine combining full-text and vector search for AI/RAG applications

License:        MIT
URL:            https://github.com/mikalv/prism
Source0:        https://github.com/mikalv/prism/archive/v%{version}.tar.gz#/prism-%{version}.tar.gz

BuildRequires:  cargo >= 1.75
BuildRequires:  rust >= 1.75

%description
Prism is a hybrid search engine combining full-text search (via Tantivy)
and vector search (HNSW) with support for embedding providers, pipelines,
and MCP protocol integration.

Includes prism-server (HTTP API), prism (CLI), and prism-import (bulk
data importer).

%prep
%autosetup -n prism-%{version}

%build
cargo build --release --workspace

%install
install -Dm755 target/release/prism-server %{buildroot}%{_bindir}/prism-server
install -Dm755 target/release/prism %{buildroot}%{_bindir}/prism
install -Dm755 target/release/prism-import %{buildroot}%{_bindir}/prism-import

%files
%license LICENSE
%{_bindir}/prism-server
%{_bindir}/prism
%{_bindir}/prism-import

%changelog
* Thu Feb 13 2026 mikalv <spam@mux.rs> - 0.6.2-1
- Initial RPM packaging
