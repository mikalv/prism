import re

with open('prism/tests/e2e_api_test.rs', 'r') as f:
    content = f.read()

# Replace assert_eq!(resp.status().as_u16(), 200); with a panic if not 200 that prints the body, but without consuming resp if we just clone the status or do it after we get the body. Wait, no, we can't consume resp if we want to parse it as json later.
# We can do:
# let status = resp.status().as_u16();
# let bytes = resp.bytes().await.unwrap();
# if status != 200 { panic!("Error: {}", String::from_utf8_lossy(&bytes)); }
# let body: Value = serde_json::from_slice(&bytes).unwrap();

# Actually, I'll just change line 1449.

target = """    assert_eq!(resp.status().as_u16(), 200);

    let body: Value = resp.json().await.unwrap();"""

replacement = """    let status = resp.status().as_u16();
    let bytes = resp.bytes().await.unwrap();
    if status != 200 {
        panic!("Status was {}, body: {}", status, String::from_utf8_lossy(&bytes));
    }
    let body: Value = serde_json::from_slice(&bytes).unwrap();"""

new_content = content.replace(target, replacement)
with open('prism/tests/e2e_api_test.rs', 'w') as f:
    f.write(new_content)
