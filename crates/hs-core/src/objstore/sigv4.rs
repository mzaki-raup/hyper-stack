//! AWS Signature Version 4 (header signing and presigned query strings),
//! written for HyperStack and checked against AWS's published examples.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// SHA-256 of an empty body.
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

#[derive(Clone)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret.
        f.debug_struct("Credentials")
            .field("access_key", &self.access_key)
            .finish_non_exhaustive()
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC takes any key length");
    m.update(data);
    m.finalize().into_bytes().to_vec()
}

/// URI-encode per SigV4: every byte except `A-Z a-z 0-9 - . _ ~` (and `/`
/// unless `encode_slash`) becomes `%XX` with upper-case hex.
pub fn uri_encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            b'/' if !encode_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Sorted, encoded `k=v&…` (both keys and values encoded, `/` too).
pub fn canonical_query(pairs: &[(String, String)]) -> String {
    let mut enc: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
        .collect();
    enc.sort();
    enc.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// A request to sign. `path` is the canonical (already encoded) URI path;
/// `headers` must include `host` and every header that is sent signed.
pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a [(String, String)],
    pub headers: &'a [(String, String)],
    pub payload_hash: &'a str,
}

fn scope(date: &str, region: &str, service: &str) -> String {
    format!("{}/{region}/{service}/aws4_request", &date[..8])
}

fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), &date.as_bytes()[..8]);
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, service.as_bytes());
    hmac(&k_service, b"aws4_request")
}

/// Lower-cased, trimmed, sorted headers: (canonical block, signed list).
fn canonical_headers(headers: &[(String, String)]) -> (String, String) {
    let mut h: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| {
            (
                k.to_ascii_lowercase(),
                v.split_whitespace().collect::<Vec<_>>().join(" "),
            )
        })
        .collect();
    h.sort();
    let block: String = h.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = h
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    (block, signed)
}

fn signature(
    creds: &Credentials,
    region: &str,
    service: &str,
    amz_date: &str,
    req: &Request,
) -> (String, String) {
    let (block, signed) = canonical_headers(req.headers);
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        req.method,
        req.path,
        canonical_query(req.query),
        block,
        signed,
        req.payload_hash
    );
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{}\n{}",
        scope(amz_date, region, service),
        sha256_hex(canonical.as_bytes())
    );
    let key = signing_key(&creds.secret_key, amz_date, region, service);
    (hex(&hmac(&key, to_sign.as_bytes())), signed)
}

/// The `Authorization` header value for `req` (whose headers already carry
/// `x-amz-date`, and `x-amz-content-sha256` for S3).
pub fn authorization(
    creds: &Credentials,
    region: &str,
    service: &str,
    amz_date: &str,
    req: &Request,
) -> String {
    let (sig, signed) = signature(creds, region, service, amz_date, req);
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={signed}, Signature={sig}",
        creds.access_key,
        scope(amz_date, region, service)
    )
}

/// What a presigned URL grants: `method` on `host` + `path` (canonical,
/// encoded), with extra `query` parameters that are signed too.
pub struct Presign<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub query: &'a [(String, String)],
    pub expires_secs: u64,
}

/// Query parameters for a presigned URL (only `host` signed, payload
/// unsigned), including `X-Amz-Signature`.
pub fn presign_query(
    creds: &Credentials,
    region: &str,
    amz_date: &str,
    p: &Presign,
) -> Vec<(String, String)> {
    let (method, host, path, expires_secs) = (p.method, p.host, p.path, p.expires_secs);
    let mut q: Vec<(String, String)> = p.query.to_vec();
    q.push(("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()));
    q.push((
        "X-Amz-Credential".into(),
        format!("{}/{}", creds.access_key, scope(amz_date, region, "s3")),
    ));
    q.push(("X-Amz-Date".into(), amz_date.into()));
    q.push(("X-Amz-Expires".into(), expires_secs.to_string()));
    if let Some(t) = &creds.session_token {
        q.push(("X-Amz-Security-Token".into(), t.clone()));
    }
    q.push(("X-Amz-SignedHeaders".into(), "host".into()));
    let headers = [("host".to_string(), host.to_string())];
    let (sig, _) = signature(
        creds,
        region,
        "s3",
        amz_date,
        &Request {
            method,
            path,
            query: &q,
            headers: &headers,
            payload_hash: UNSIGNED_PAYLOAD,
        },
    );
    q.push(("X-Amz-Signature".into(), sig));
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    fn s3_example_creds() -> Credentials {
        Credentials {
            access_key: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        }
    }

    /// aws-sig-v4-test-suite: get-vanilla.
    #[test]
    fn sigv4_test_suite_get_vanilla() {
        let creds = Credentials {
            access_key: "AKIDEXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
        };
        let headers = [
            h("Host", "example.amazonaws.com"),
            h("X-Amz-Date", "20150830T123600Z"),
        ];
        let auth = authorization(
            &creds,
            "us-east-1",
            "service",
            "20150830T123600Z",
            &Request {
                method: "GET",
                path: "/",
                query: &[],
                headers: &headers,
                payload_hash: EMPTY_SHA256,
            },
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    /// S3 docs, "Example: GET Object" (range request).
    #[test]
    fn s3_get_object_example() {
        let headers = [
            h("Host", "examplebucket.s3.amazonaws.com"),
            h("Range", "bytes=0-9"),
            h("x-amz-content-sha256", EMPTY_SHA256),
            h("x-amz-date", "20130524T000000Z"),
        ];
        let auth = authorization(
            &s3_example_creds(),
            "us-east-1",
            "s3",
            "20130524T000000Z",
            &Request {
                method: "GET",
                path: "/test.txt",
                query: &[],
                headers: &headers,
                payload_hash: EMPTY_SHA256,
            },
        );
        assert!(
            auth.ends_with(
                "SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
                 Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
            ),
            "{auth}"
        );
    }

    /// S3 docs, "Example: PUT Object" (key with `$`, extra headers).
    #[test]
    fn s3_put_object_example() {
        let body = b"Welcome to Amazon S3.";
        let hash = sha256_hex(body);
        assert_eq!(
            hash,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        let headers = [
            h("Host", "examplebucket.s3.amazonaws.com"),
            h("Date", "Fri, 24 May 2013 00:00:00 GMT"),
            h("x-amz-date", "20130524T000000Z"),
            h("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            h("x-amz-content-sha256", &hash),
        ];
        let path = format!("/{}", uri_encode("test$file.text", false));
        assert_eq!(path, "/test%24file.text");
        let auth = authorization(
            &s3_example_creds(),
            "us-east-1",
            "s3",
            "20130524T000000Z",
            &Request {
                method: "PUT",
                path: &path,
                query: &[],
                headers: &headers,
                payload_hash: &hash,
            },
        );
        assert!(
            auth.ends_with(
                "Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
            ),
            "{auth}"
        );
    }

    /// S3 docs, "Example: presigned URL" (24 h GET).
    #[test]
    fn s3_presigned_url_example() {
        let q = presign_query(
            &s3_example_creds(),
            "us-east-1",
            "20130524T000000Z",
            &Presign {
                method: "GET",
                host: "examplebucket.s3.amazonaws.com",
                path: "/test.txt",
                query: &[],
                expires_secs: 86400,
            },
        );
        let sig = q.iter().find(|(k, _)| k == "X-Amz-Signature").unwrap();
        assert_eq!(
            sig.1,
            "aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
        );
    }

    #[test]
    fn encoding_rules() {
        assert_eq!(uri_encode("a b/c+d~e", false), "a%20b/c%2Bd~e");
        assert_eq!(uri_encode("a/b", true), "a%2Fb");
        assert_eq!(uri_encode("é", true), "%C3%A9");
        assert_eq!(
            canonical_query(&[
                h("prefix", "a b/"),
                h("delimiter", "/"),
                h("list-type", "2")
            ]),
            "delimiter=%2F&list-type=2&prefix=a%20b%2F"
        );
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let s = format!("{:?}", s3_example_creds());
        assert!(!s.contains("EXAMPLEKEY"), "{s}");
    }
}
