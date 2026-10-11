//! The S3 client against a real S3-compatible server. Runs only when
//! `HS_S3_ENDPOINT` is set (for example `http://127.0.0.1:5055` with
//! `moto_server -p 5055`, or a MinIO with `HS_S3_ACCESS_KEY`/`HS_S3_SECRET_KEY`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use hs_core::objstore::s3::{S3Client, S3Config, Versioning};
use hs_core::objstore::sigv4::Credentials;
use tokio_util::sync::CancellationToken;

fn client() -> Option<S3Client> {
    let endpoint = std::env::var("HS_S3_ENDPOINT").ok()?;
    let var = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    Some(
        S3Client::new(S3Config {
            endpoint,
            region: var("HS_S3_REGION", "us-east-1"),
            path_style: true,
            credentials: Credentials {
                access_key: var("HS_S3_ACCESS_KEY", "testing"),
                secret_key: var("HS_S3_SECRET_KEY", "testing"),
                session_token: None,
            },
        })
        .unwrap(),
    )
}

#[test]
fn s3_round_trip_against_a_live_server() {
    let Some(c) = client() else {
        eprintln!("HS_S3_ENDPOINT not set; skipping");
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let bucket = format!("hs-test-{}", hs_core::new_id().to_lowercase());
        c.create_bucket(&bucket).await.unwrap();
        assert!(
            c.list_buckets()
                .await
                .unwrap()
                .iter()
                .any(|b| b.name == bucket)
        );

        // Small objects, odd keys, folders by delimiter.
        for key in [
            "docs/readme.md",
            "docs/a b+c.txt",
            "img/2026/x.png",
            "root.json",
        ] {
            c.put_bytes(&bucket, key, key.as_bytes().to_vec(), "text/plain")
                .await
                .unwrap();
        }
        let top = c.list(&bucket, "", true, None, 1000).await.unwrap();
        assert_eq!(top.prefixes, vec!["docs/", "img/"]);
        assert_eq!(top.objects.len(), 1);
        let docs = c.list(&bucket, "docs/", true, None, 1000).await.unwrap();
        let keys: Vec<&str> = docs.objects.iter().map(|o| o.key.as_str()).collect();
        assert_eq!(keys, vec!["docs/a b+c.txt", "docs/readme.md"]);
        assert_eq!(
            c.get_bytes(&bucket, "docs/a b+c.txt").await.unwrap(),
            b"docs/a b+c.txt"
        );

        // Paging with continuation tokens.
        let p1 = c.list(&bucket, "", false, None, 2).await.unwrap();
        assert_eq!(p1.objects.len(), 2);
        let p2 = c
            .list(&bucket, "", false, p1.next.as_deref(), 2)
            .await
            .unwrap();
        assert_eq!(p2.objects.len(), 2);
        assert!(p2.next.is_none());

        // Multipart upload (20 MiB in 8 MiB parts), head, download, compare.
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("big.bin");
        let data: Vec<u8> = (0..20 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        let seen = Arc::new(AtomicU64::new(0));
        let s2 = seen.clone();
        c.upload_file(
            &bucket,
            "big/big.bin",
            &src,
            Arc::new(move |done, _| s2.store(done, Ordering::Relaxed)),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(seen.load(Ordering::Relaxed), data.len() as u64);
        let head = c.head(&bucket, "big/big.bin").await.unwrap();
        assert_eq!(head.size, data.len() as u64);
        assert!(head.etag.ends_with("-3"), "multipart etag: {}", head.etag);
        let dest = dir.path().join("back.bin");
        c.download(
            &bucket,
            "big/big.bin",
            &dest,
            Arc::new(|_, _| {}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);

        // A presigned GET works without credentials.
        let url = c.presign("GET", &bucket, "root.json", 600);
        let body = reqwest::get(&url).await.unwrap();
        assert!(body.status().is_success(), "{}", body.status());
        assert_eq!(body.text().await.unwrap(), "root.json");

        assert_eq!(c.versioning(&bucket).await.unwrap(), Versioning::Never);

        // Delete a prefix in one batch, then everything.
        let doc_keys = c.list_all_keys(&bucket, "docs/").await.unwrap();
        assert_eq!(doc_keys.len(), 2);
        assert!(c.delete_many(&bucket, &doc_keys).await.unwrap().is_empty());
        assert!(c.list_all_keys(&bucket, "docs/").await.unwrap().is_empty());
        let rest = c.list_all_keys(&bucket, "").await.unwrap();
        assert!(c.delete_many(&bucket, &rest).await.unwrap().is_empty());
        let err = c.head(&bucket, "root.json").await.unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
        c.delete_bucket(&bucket).await.unwrap();
        assert!(
            !c.list_buckets()
                .await
                .unwrap()
                .iter()
                .any(|b| b.name == bucket)
        );
    });
}
