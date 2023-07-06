use std::env;

use axum::{
    extract,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{self, types::BigDecimal, PgPool};
use tracing::info;

use crate::{
    error::{Error, InteractionError},
    utils::serialize_bigdecimal,
    DatabaseCommand, TX,
};

pub fn create_route() -> Router {
    Router::new()
        .route("/products", get(get_products))
        .route("/products", post(process_checkout))
}

#[derive(Serialize, Deserialize, Debug, sqlx::FromRow)]
pub struct Product {
    id: i32,
    name: String,
    href: String,
    #[serde(serialize_with = "serialize_bigdecimal")]
    price: BigDecimal,
    description: String,
    #[serde(rename = "imageSrc")]
    image_src: String,
    #[serde(rename = "imageAlt")]
    image_alt: String,
}

async fn get_products() -> Result<Json<Vec<Product>>, Error> {
    let database_url = env::var("DATABASE_URL").unwrap();
    let pool = PgPool::connect(&database_url).await?;

    let products: Vec<Product> = sqlx::query_as(
        r#"
        SELECT id, name, href, ROUND(price::numeric, 2) AS price, description, image_src, image_alt
        FROM products
        "#,
    )
    .fetch_all(&pool)
    .await?;

    println!("{:?}", products);
    Ok(Json(products))
}

#[derive(Debug, Serialize)]
struct Medication {
    medications: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    pub message: String,
    pub interactions: Option<Vec<Vec<String>>>,
}

async fn process_checkout(
    extract::Json(candidates): extract::Json<Vec<i32>>,
) -> Result<Json<bool>, Error> {
    let database_url = env::var("DATABASE_URL")?;
    let token = env::var("LAMBDA_TOKEN")?;
    let url = env::var("LAMBDA_URL")?;
    let pool = PgPool::connect(&database_url).await?;
    let sql = sqlx::query_as::<_, (String,)>("SELECT name FROM PRODUCTS WHERE ID = ANY($1)")
        .bind(&candidates)
        .fetch_all(&pool)
        .await?;
    let data: Vec<String> = sql.iter().map(|p| p.0.clone()).collect();
    let payload = Medication {
        medications: data.clone(),
    };

    let client = reqwest::Client::new();
    let response: Message = client
        .post(url)
        .header("X-Gravitee-Api-Key", token)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;
    if response.interactions.is_some() {
        let error = InteractionError {
            message: response.message,
            interactions: response.interactions.expect("is interaction"),
        };
        info!("{:?}", error);
        return Err(Error::InteractionError(error));
    }

    let tx = TX.lock().unwrap();
    let _ = tx.send(DatabaseCommand::Insert(data));

    Ok(Json(true))
}
