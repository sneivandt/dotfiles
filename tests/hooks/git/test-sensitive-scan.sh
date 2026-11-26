#!/bin/sh
# shellcheck shell=sh

test_sensitive_scan_without_paste() {
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"
  repo="$(mktemp -d)"

  git init -q "$repo"
  mkdir -p "$repo/hooks" "$repo/bin"
  cp "$repo_root/hooks/check-sensitive.sh" "$repo/hooks/check-sensitive.sh"
  cp "$repo_root/hooks/sensitive-patterns.ini" "$repo/hooks/sensitive-patterns.ini"
  cp "$repo_root/hooks/sensitive-allowlist.ini" "$repo/hooks/sensitive-allowlist.ini"

  cat > "$repo/bin/paste" <<'EOF'
#!/bin/sh
exit 127
EOF
  chmod +x "$repo/bin/paste"

  printf '%s\n' 'release = v2026.07.30-1' > "$repo/test-file.txt"
  git -C "$repo" add test-file.txt

  if (
    cd "$repo"
    PATH="$repo/bin:$PATH" sh hooks/check-sensitive.sh >/dev/null 2>&1
  ); then
    pass "Sensitive-data scan does not require paste"
  else
    fail "Sensitive-data scan requires unavailable paste"
  fi

  rm -rf "$repo"
}

test_sensitive_scan_unusual_paths() {
  key_header=$(printf '%s%s' '-----BEGIN ' 'PRIVATE KEY-----')
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"
  repo="$(mktemp -d)"
  git init -q --template= "$repo"
  git -C "$repo" config user.name "Hook Test"
  git -C "$repo" config user.email "ci@test.local"
  mkdir -p "$repo/hooks"
  cp "$repo_root/hooks/check-sensitive.sh" "$repo/hooks/"
  cp "$repo_root/hooks/sensitive-patterns.ini" "$repo/hooks/"
  cp "$repo_root/hooks/sensitive-allowlist.ini" "$repo/hooks/"
  printf 'baseline\n' > "$repo/baseline"
  git -C "$repo" add baseline
  git -C "$repo" -c core.hooksPath=/dev/null commit -qm baseline

  for file in 'space name.txt' "$(printf 'tab\tname.txt')" \
    "$(printf 'line\nname.txt')" 'back\slash.txt' '-leading-dash.txt'; do
    printf 'safe line\n%s\n' "$key_header" > "$repo/$file"
    git -C "$repo" add -- "$file"
    if scan_output=$(cd "$repo" && sh hooks/check-sensitive.sh 2>&1); then
      fail "Sensitive scan allowed content in an unusual filename"
    elif ! printf '%s\n' "$scan_output" | grep -Fq 'Line 2: <redacted>'; then
      fail "Sensitive scan did not report the correct line in an unusual filename"
    else
      pass "Sensitive scan blocks content in an unusual filename"
    fi
    git -C "$repo" reset -q HEAD -- "$file"
    rm -- "$repo/$file"
  done

  printf 'safe line\n%s\n' "$(printf '%s%s' '-----BEGIN RSA ' 'PRIVATE KEY-----')" > "$repo/rsa-key.txt"
  git -C "$repo" add rsa-key.txt
  if scan_output=$(cd "$repo" && sh hooks/check-sensitive.sh 2>&1); then
    fail "Sensitive scan allowed an RSA private key header"
  elif [ "$(printf '%s\n' "$scan_output" | grep -Fc 'Line 2: <redacted>')" -ne 1 ]; then
    fail "Sensitive scan reported duplicate matches for a single RSA private key header"
  else
    pass "Sensitive scan reports a single finding for an RSA private key header"
  fi
  git -C "$repo" reset -q HEAD -- rsa-key.txt
  rm -- "$repo/rsa-key.txt"

  printf 'safe line\n%s\n' "$key_header" > "$repo/first.txt"
  printf 'safe line\nanother safe line\n++%s\n' "$key_header" > "$repo/second space.txt"
  git -C "$repo" add -- first.txt 'second space.txt'
  if scan_output=$(cd "$repo" && sh hooks/check-sensitive.sh 2>&1); then
    fail "Sensitive scan allowed additions across multiple files"
  elif ! printf '%s\n' "$scan_output" | grep -Fq 'In file: first.txt' ||
       ! printf '%s\n' "$scan_output" | grep -Fq 'In file: second space.txt' ||
       ! printf '%s\n' "$scan_output" | grep -Fq 'Line 2: <redacted>' ||
       ! printf '%s\n' "$scan_output" | grep -Fq 'Line 3: <redacted>'; then
    fail "Sensitive scan lost filenames, line numbers or ++ prefixed content"
  else
    pass "Sensitive scan reports additions across multiple files, including ++ content"
  fi
  git -C "$repo" reset -q HEAD -- first.txt 'second space.txt'
  rm -- "$repo/first.txt" "$repo/second space.txt"

  old="$(printf 'old\tname.txt')"
  new="$(printf 'new\nname.txt')"
  # Enough unchanged content to exercise Git's actual rename detection.
  seq 1 30 | sed 's/^/safe line /' > "$repo/$old"
  git -C "$repo" add -- "$old"
  git -C "$repo" -c core.hooksPath=/dev/null commit -qm 'rename baseline'
  mv -- "$repo/$old" "$repo/$new"
  printf '%s\n' "$key_header" >> "$repo/$new"
  git -C "$repo" add -A -- "$old" "$new"
  if ! git -C "$repo" diff --cached --summary | grep -q rename; then
    fail "Unusual-path fixture did not produce a rename"
  elif scan_output=$(cd "$repo" && sh hooks/check-sensitive.sh 2>&1); then
    fail "Sensitive scan allowed new content in a renamed file"
  elif ! printf '%s\n' "$scan_output" | grep -Fq 'Line 31: <redacted>'; then
    fail "Sensitive scan did not report the renamed file's added line"
  else
    pass "Sensitive scan blocks new content in a renamed unusual path"
  fi

  git -C "$repo" -c core.hooksPath=/dev/null commit -qm 'deletion baseline'
  rm -- "$repo/$new"
  git -C "$repo" add -A -- "$new"
  if (cd "$repo" && sh hooks/check-sensitive.sh >/dev/null 2>&1); then
    pass "Sensitive scan allows deletion of sensitive content in an unusual path"
  else
    fail "Sensitive scan blocked a deletion"
  fi
  rm -rf "$repo"
}

test_sensitive_scan_errors() {
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"
  repo="$(mktemp -d)"
  git init -q --template= "$repo"
  mkdir -p "$repo/hooks" "$repo/bin"
  cp "$repo_root/hooks/check-sensitive.sh" "$repo/hooks/"
  printf '(unterminated\n' > "$repo/hooks/sensitive-patterns.ini"

  if scan_output=$(cd "$repo" && sh hooks/check-sensitive.sh 2>&1); then
    fail "Sensitive scan accepted an invalid rule with an empty diff"
  elif printf '%s\n' "$scan_output" | grep -q 'Cannot validate'; then
    pass "Sensitive scan rejects invalid rules even with an empty diff"
  else
    fail "Sensitive scan failed without explaining the invalid rule"
  fi

  printf 'sensitive-marker\n' > "$repo/hooks/sensitive-patterns.ini"
  printf '(unterminated\n' > "$repo/hooks/sensitive-allowlist.ini"
  if (cd "$repo" && sh hooks/check-sensitive.sh >/dev/null 2>&1); then
    fail "Sensitive scan accepted an invalid allowlist with an empty diff"
  else
    pass "Sensitive scan rejects an invalid allowlist with an empty diff"
  fi
  rm "$repo/hooks/sensitive-allowlist.ini"

  printf 'safe content\n' > "$repo/test-file.txt"
  git -C "$repo" add test-file.txt
  cat > "$repo/bin/grep" <<'EOF'
#!/bin/sh
# Preflight succeeds with no match, but scanning the content fails.
for argument do
  if [ "$argument" = /dev/null ]; then
    exit 1
  fi
done
exit 2
EOF
  chmod +x "$repo/bin/grep"
  if scan_output=$(cd "$repo" && PATH="$repo/bin:$PATH" sh hooks/check-sensitive.sh 2>&1); then
    fail "Sensitive scan ignored a grep execution error"
  elif printf '%s\n' "$scan_output" | grep -q 'scan failed'; then
    pass "Sensitive scan reports and rejects grep execution errors"
  else
    fail "Sensitive scan failed without explaining the grep error"
  fi
  rm "$repo/bin/grep"

  cat > "$repo/bin/git" <<'EOF'
#!/bin/sh
case "$1" in
  rev-parse) exit 1 ;;
  hash-object) printf 'unused-tree\n' ;;
  diff) exit 2 ;;
  *) exit 2 ;;
esac
EOF
  chmod +x "$repo/bin/git"
  if (cd "$repo" && PATH="$repo/bin:$PATH" sh hooks/check-sensitive.sh >/dev/null 2>&1); then
    fail "Sensitive scan ignored a Git diff error"
  else
    pass "Sensitive scan rejects Git diff errors"
  fi
  rm -rf "$repo"
}

run_sensitive_pattern_tests() {
# Construct synthetic test fixtures dynamically so the test file itself stays
# clean under the repository's pre-commit sensitive-content scanner.

# Test AWS credentials
printf "Testing AWS patterns...\n"
sample_aws=$(printf '%s%s' 'AKIAIOSFODNN' '7EXAMPLE')
sample_aws_line=$(printf '%s_%s = %s' 'aws_access' 'key_id' "$sample_aws")
test_hook_blocks "AWS access key" "$sample_aws_line"
test_hook_blocks "AWS key ID format" "export AWS_KEY=$sample_aws"

# Test GitHub tokens
printf "\nTesting GitHub patterns...\n"
sample_ghp=$(printf 'ghp_%s%s' '12345678901234567890' '12345678901234567890')
sample_gho=$(printf 'gho_%s%s' 'abcdefghijklmnopqrstuvwx' 'yz12345678901234')
test_hook_blocks "GitHub PAT" "token = $sample_ghp"
test_hook_blocks "GitHub token assignment" "GITHUB_TOKEN=$sample_gho"
fine_grained_pat="github_pat_$(printf '%s' 'abcdefghijklmnopqrstuv')"
test_hook_blocks "GitHub fine-grained PAT" "GITHUB_TOKEN=$fine_grained_pat"

# Test API keys
printf "\nTesting API key patterns...\n"
sample_hex_key=$(printf '%s%s' '1234567890abcdef' '1234567890abcdef')
sample_secret_key=$(printf '%s%s' 'abcdef1234567890' 'abcdef1234567890')
test_hook_blocks "API key" "apikey = \"$sample_hex_key\""
single_quoted_api_key=$(printf "apikey = '%s'" "$sample_hex_key")
test_hook_blocks "API key (single-quoted)" "$single_quoted_api_key"
test_hook_blocks "Secret key" "secret_key: \"$sample_secret_key\""
test_hook_blocks_staged_file_deleted_from_worktree

# Test passwords
printf "\nTesting password patterns...\n"
sample_pass=$(printf '%s%s' 'MySecret' 'Pass123')
sample_passwd=$(printf '%s%s' 'Another' 'Password456')
test_hook_blocks "Password" "password = \"$sample_pass\""
single_quoted_password=$(printf "password = '%s'" "$sample_pass")
test_hook_blocks "Password (single-quoted)" "$single_quoted_password"
test_hook_blocks "Passwd" "passwd: \"$sample_passwd\""

# Test bearer and authorization tokens
printf "\nTesting bearer and authorization patterns...\n"
sample_jwt="$(printf '%s.%s.%s' 'eyJhbGciOiJIUzI1NiJ9' 'eyJzdWIiOiJ0ZXN0In0' 'dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk')"
sample_val="$(printf '%s%s' 'abc123+def456/' 'ghi789==abcd')"
test_hook_blocks "Authorization Bearer JWT" "Authorization: Bearer $sample_jwt"
test_hook_blocks "Quoted bearer assignment" "bearer = \"$sample_val\""
test_hook_blocks "Quoted authorization Bearer" "authorization: \"Bearer $sample_val\""

# Test private keys
printf "\nTesting private key patterns...\n"
sample_rsa_key=$(printf '%s %s %s' '-----BEGIN' 'RSA PRIVATE' 'KEY-----')
sample_ssh_key=$(printf '%s %s %s' '-----BEGIN' 'OPENSSH PRIVATE' 'KEY-----')
test_hook_blocks "RSA private key" "$sample_rsa_key"
test_hook_blocks "OpenSSH private key" "$sample_ssh_key"

# Test database connection strings
printf "\nTesting database patterns...\n"
sample_mysql=$(printf '%s://%s:%s%s' 'mysql' 'user' 'password' '@localhost:3306/db')
sample_pg=$(printf '%s://%s:%s%s' 'postgresql' 'admin' 'secret' '@db.example.com/mydb')
test_hook_blocks "MySQL connection" "$sample_mysql"
test_hook_blocks "PostgreSQL connection" "$sample_pg"

# Test Heroku API keys (UUID format with context)
printf "\nTesting Heroku patterns...\n"
sample_heroku_uuid=$(printf '%s-%s-%s-%s-%s' '550e8400' 'e29b' '41d4' 'a716' '446655440000')
test_hook_blocks "Heroku API key" "HEROKU_API_KEY=$sample_heroku_uuid"

# Test Slack tokens
printf "\nTesting Slack patterns...\n"
sample_slack=$(printf '%s-%s-%s-%s' 'xoxb' '1111111111111' '2222222222222' 'EXAMPLEEXAMPLEEXAMPLEEXA')
test_hook_blocks "Slack bot token" "SLACK_TOKEN=$sample_slack"

# Test Stripe keys
printf "\nTesting Stripe patterns...\n"
sample_stripe=$(printf '%s_%s_%s' 'sk' 'test' 'EXAMPLEKEY1234567890abcd')
test_hook_blocks "Stripe secret key" "STRIPE_SECRET=$sample_stripe"

# Test Google API keys
printf "\nTesting Google patterns...\n"
sample_google=$(printf '%s%s' 'AIzaSyC1234567890' 'abcdefghijklmnopqrstuvw')
test_hook_blocks "Google API key" "GOOGLE_API_KEY=$sample_google"

# Test GitLab tokens
printf "\nTesting GitLab patterns...\n"
sample_gitlab=$(printf '%s-%s' 'glpat' 'abcdefghijklmnopqrstuvwxyz')
test_hook_blocks "GitLab PAT" "GITLAB_TOKEN=$sample_gitlab"

# Test OAuth secrets
printf "\nTesting OAuth patterns...\n"
test_hook_blocks "OAuth client secret" "client_secret = \"$sample_hex_key\""

# Test generic high-entropy secrets
printf "\nTesting generic patterns...\n"
sample_generic=$(printf '%s%s' 'a1b2c3d4e5f6g7h8i9j0' 'k1l2m3n4o5p6q7r8s9t0')
test_hook_blocks "Generic secret" "secret = \"$sample_generic\""

# Test UUID/GUID patterns
printf "\nTesting UUID patterns...\n"
sample_uuid_a=$(printf '%s-%s-%s-%s-%s' '12345678' '1234' '1234' '1234' '123456789abc')
sample_uuid_b=$(printf '%s-%s-%s-%s-%s' 'abcdef12' '3456' '7890' 'abcd' 'ef1234567890')
sample_uuid_c=$(printf '%s-%s-%s-%s-%s' '11111111' '2222' '3333' '4444' '555555555555')
test_hook_blocks "UUID in URL" "https://example.com/auth/$sample_uuid_a"
test_hook_blocks "GUID assignment" "tenant_id = $sample_uuid_b"
test_hook_blocks "UUID constant" "const CLIENT_ID = '$sample_uuid_c'"

# Test PII patterns
printf "\nTesting PII patterns...\n"
sample_email=$(printf '%s@%s' 'john.doe' 'example.com')
sample_phone_fmt=$(printf '(%s) %s-%s' '555' '123' '4567')
sample_phone_dash=$(printf '%s-%s-%s' '555' '123' '4567')
sample_phone_plain=$(printf '%s%s%s' '555' '123' '4567')
sample_phone=$(printf '+1-%s-%s-%s' '555' '123' '4567')
sample_ssn=$(printf '%s-%s-%s' '123' '45' '6789')
sample_ssn_plain=$(printf '%s%s%s' '123' '45' '6789')
sample_cc_spaces=$(printf '%s %s %s %s' '4532' '1234' '5678' '9010')
sample_cc_dashes=$(printf '%s-%s-%s-%s' '4532' '1234' '5678' '9010')
sample_ipv4=$(printf '%s.%s.%s.%s' '192' '168' '1' '100')
sample_ipv6=$(printf '%s:%s:%s:%s:%s:%s:%s:%s' '2001' '0db8' '85a3' '0000' '0000' '8a2e' '0370' '7334')
test_hook_blocks "Email address" "user_email = $sample_email"
test_hook_blocks "Phone number (formatted)" "phone = $sample_phone_fmt"
test_hook_blocks "Phone number (dashes)" "contact = $sample_phone_dash"
test_hook_blocks "Phone number (plain)" "phone_number = $sample_phone_plain"
test_hook_blocks "Phone number (+1 country code)" "phone = $sample_phone"
test_hook_blocks "SSN (formatted)" "ssn = $sample_ssn"
test_hook_blocks "SSN (plain)" "social_security_number = $sample_ssn_plain"
test_hook_blocks "Credit card (spaces)" "cc = $sample_cc_spaces"
test_hook_blocks "Credit card (dashes)" "card_number = $sample_cc_dashes"
test_hook_blocks "IPv4 address" "server_ip = $sample_ipv4"
test_hook_blocks "IPv6 address" "ipv6 = $sample_ipv6"
test_hook_blocks "Commit SHA alongside real PII" "\"commit\": \"64a654ef4a20db1727938338310209b6a63f60c9\", \"ssn\": \"$sample_ssn\""

# Test clean commits (should be allowed)
printf "\nTesting clean commits...\n"
test_hook_allows "# This is a clean comment"
test_hook_allows "const API_URL = 'https://api.example.com'"
test_hook_allows "password_field = 'password'  # Field name, not actual password"
test_hook_allows "const SECRET_LENGTH = 32  # Configuration constant"
test_hook_allows '"commit": "64a654ef4a20db1727938338310209b6a63f60c9"'

}
