find . -name "*.rs" -type f -print0 | xargs -0 perl -pi -e 's/(let\s+[a-zA-Z0-9_]+\s*=\s*(?:crate::backends::)?Query\s*\{)/\1
        vector: None,/g'
