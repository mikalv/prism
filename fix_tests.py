import glob
import re

for filename in glob.glob('prism/tests/*.rs'):
    with open(filename, 'r') as f:
        lines = f.readlines()
        
    out_lines = []
    i = 0
    while i < len(lines):
        line = lines[i]
        
        # Check for Query {
        if re.search(r'\bQuery\s*\{', line) and '->' not in line:
            # Check if next line has vector:
            if i + 1 < len(lines) and 'vector:' not in lines[i+1]:
                out_lines.append(line)
                indent = re.match(r'^\s*', lines[i+1]).group(0)
                out_lines.append(indent + 'vector: None,\n')
                i += 1
                continue
                
        # Check for VectorBackendConfig {
        if re.search(r'VectorBackendConfig\s*\{', line):
            if i + 1 < len(lines) and 'wal:' not in lines[i+1]:
                out_lines.append(line)
                indent = re.match(r'^\s*', lines[i+1]).group(0)
                out_lines.append(indent + 'wal: prism::schema::types::WalConfig::default(),\n')
                i += 1
                continue

        # Check for merge_weighted_with_normalization
        if 'merge_weighted_with_normalization(' in line:
            # Skip forward until we find ScoreNormalization
            out_lines.append(line)
            j = i + 1
            args = []
            while j < len(lines) and '&ScoreNormalization::' not in lines[j]:
                args.append(lines[j])
                j += 1
            if j < len(lines) and '&ScoreNormalization::' in lines[j]:
                # We found ScoreNormalization.
                # In advanced_ranking_test, we need to insert '0,' before the limit (which is the last arg in args)
                # But only if args is exactly 4 lines (text, vector, weight, weight, limit) -> wait, there are 5 args before offset
                # Let's just match the limit line
                # It's always 10, or 20,
                # Actually let's just insert '0,' before the ScoreNormalization line if it's advanced_ranking_test.
                if 'advanced_ranking_test.rs' in filename:
                    # Let's check if the previous line was a number (limit)
                    # We can just insert '        0,\n' before lines[j]
                    # But wait, did we already insert it? Let's check if args[-1] has 0,
                    # We can just check the number of arguments passed before ScoreNormalization.
                    # It expects: text, vector, text_weight, vec_weight, offset, limit. (6 args before norm)
                    # Currently it has: text, vector, text_weight, vec_weight, limit. (5 args)
                    if len(args) == 5:
                        args.append(re.match(r'^\s*', lines[j]).group(0) + '0,\n')
                out_lines.extend(args)
                out_lines.append(lines[j])
                i = j + 1
                continue

        out_lines.append(line)
        i += 1
        
    with open(filename, 'w') as f:
        f.writelines(out_lines)
