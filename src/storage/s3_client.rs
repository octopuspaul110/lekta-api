use aws_sdk_s3::{Client, presigning::{ PresigningConfig }};
use std::time::Duration;

use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct S3Client {
    client: Client,
    bucket: String,
}

impl S3Client {
    pub async fn new(bucket: String, region: String) -> AppResult<Self> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new(region))
        .load()
        .await;
        let client = Client::new(&config);
        Ok(Self { client, bucket })
    }

    /// Generates a presigned URL for uploadidng (PUT) a file.
    pub async fn presign_upload(
        &self,
        key: &str,
        content_type: &str,
        expires_in: Duration,
    ) -> AppResult<String> {
        let presigning = PresigningConfig::expires_in(expires_in)
            .map_err(|e| AppError::Internal(format!("presign config: {}", e)))?;

        let presigned = self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .presigned(presigning)
            .await
            .map_err(|e| AppError::Internal(format!("presign PUT: {}", e)))?;

        Ok(presigned.uri().to_string())
    }

    /// Generates a presigned URL for uploading (PUT) a file.
    pub async fn presign_download(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> AppResult<String> {
        let presigning = PresigningConfig::expires_in(expires_in)
            .map_err(|e| AppError::Internal(format!("presign config: {}", e)))?;

        let presigned = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning)
            .await
            .map_err(|e| AppError::Internal(format!("presign GET: {}",e)))?;

        Ok(presigned.uri().to_string())
    }

    /// Delete an object
    pub async fn delete_object(&self,key: &str) -> AppResult<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("delete object: {}",e)))?;

        Ok(())
    } 
}