use axum::{Router, extract::Query, response::IntoResponse, routing::get};
use gdal::{
    Dataset,
    vector::{Geometry, LayerAccess},
};
use proj::Proj;
use serde::Deserialize;
use std::path::Path;

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(health_check))
        .route("/1m-product-urls", get(search_for_1m_usgs_product_urls));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> impl IntoResponse {
    "OK"
}

#[derive(Deserialize)]
struct BboxParams {
    bbox: String,
}

async fn search_for_1m_usgs_product_urls(Query(params): Query<BboxParams>) -> impl IntoResponse {
    println!("bbox wkt: {}", params.bbox);
    let results = search_gpkg_dataset(&params.bbox).unwrap();
    let mut return_str = String::new();
    if results.len() > 0 {
        // only check first result for now - TODO: pick most recent
        let download_links = download_list_of_download_links(&results[0])
            .await
            .expect("Failed to fetch download links for requested bbox.");
        println!("Successfully grabbed list of TIF's, checking to find overlap...");

        let only_overlapping_links = find_overlapping_files(&download_links, &params.bbox);
        println!("Overlapping ones: {:?}", only_overlapping_links);
        return_str = format!("{:?}", only_overlapping_links);
    }
    return_str
}

struct USGSProductResult {
    pub product_link: String,
    pub metadata_link: String,
    pub date: String,
    pub name: String,
}

// searches 1m metadata db for overlapping projects
fn search_gpkg_dataset(bbox_wkt: &String) -> gdal::errors::Result<Vec<USGSProductResult>> {
    let bbox = Geometry::from_wkt(bbox_wkt).unwrap();
    let mut output = vec![];
    let dataset = Dataset::open(Path::new("FESM_1m.gpkg")).unwrap();
    for mut layer in dataset.layers() {
        // filter down to jsut what matches our provided bbox
        layer.set_spatial_filter(&bbox);
        for feature in layer.features() {
            output.push(USGSProductResult {
                product_link: feature
                    .field_as_string(feature.field_index("product_link").unwrap())
                    .unwrap()
                    .unwrap(),
                metadata_link: feature
                    .field_as_string(feature.field_index("metadata_link").unwrap())
                    .unwrap()
                    .unwrap(),
                date: feature
                    .field_as_string(feature.field_index("pub_date").unwrap())
                    .unwrap()
                    .unwrap(),
                name: feature
                    .field_as_string(feature.field_index("project").unwrap())
                    .unwrap()
                    .unwrap(),
            });
        }
    }

    Ok(output)
}

// result returned from gpkg lookup doesn't have https
// they also provide a link w/query params vs a direct link
const BASE_URL_STAGED_PRODUCTS: &str = "https://prd-tnm.s3.amazonaws.com/";
fn get_download_links_txt_file_url(product_link: &String) -> String {
    // first grab the 'prefix' query param from the USGS product_link
    let mut url_parts = product_link.split("prefix=");

    // extract the prefix value
    let prefix = url_parts.nth(1).unwrap_or_default();

    // construct the full URL
    format!(
        "{}{}/0_file_download_links.txt",
        BASE_URL_STAGED_PRODUCTS, prefix
    )
}

async fn download_list_of_download_links(
    usgs_result: &USGSProductResult,
) -> Result<Vec<String>, reqwest::Error> {
    println!(
        "About to fetch data for {:?} from {}\n(date: {},  metadata link: {})",
        usgs_result.name, usgs_result.product_link, usgs_result.date, usgs_result.metadata_link
    );
    // each USGS product has a .txt file hosted that's just the direct links to each TIF file in the product, split by newlines
    let target = get_download_links_txt_file_url(&usgs_result.product_link);
    let response = reqwest::get(target).await?;
    let content = response
        .text()
        .await
        .expect("Couldn't get text from downlaod links request.");
    // split txt response of newline separated strings into a vec
    let mut links_arr: Vec<String> = vec![];
    for content_line in content.split("\n").collect::<Vec<&str>>() {
        // add to output if its not an empty line
        if content_line.len() > 0 {
            links_arr.push(content_line.to_string());
        }
    }
    Ok(links_arr)
}

fn extract_coords_from_url(url: &str) -> Option<(f64, f64)> {
    let re = regex::Regex::new(r"x(\d+)y(\d+)").ok()?;
    let caps = re.captures(url)?;

    let x: f64 = caps.get(1)?.as_str().parse().ok()?;
    let y_raw: f64 = caps.get(2)?.as_str().parse().ok()?;
    // scale up Y value, it's provided with one decimal precision (X just has int precision)
    let y = y_raw / 10.0;

    Some((x, y))
}

fn lonlat_to_utm(lon: f64, lat: f64) -> Option<(f64, f64, u8)> {
    let zone = ((lon + 180.0) / 6.0).floor() as u8 + 1;
    let epsg_code = if lat >= 0.0 {
        32600 + zone as u32 // Northern Hemisphere
    } else {
        32700 + zone as u32 // Southern Hemisphere
    };
    let to_utm = Proj::new_known_crs("EPSG:4326", &format!("EPSG:{}", epsg_code), None).ok()?;
    let (x, y) = to_utm.convert((lon, lat)).ok()?;
    Some((x, y, zone))
}

fn find_overlapping_files(tif_urls: &[String], input_bbox_wkt: &String) -> Vec<String> {
    // grab input bbox
    let bbox_wkt = input_bbox_wkt.clone();
    let input_bbox_geom = Geometry::from_wkt(&bbox_wkt).unwrap();
    // get it's actual bbox (in case someone passed a non bbox polygon in)
    let input_bbox_envelope = input_bbox_geom.envelope();
    // convert bbox points to UTM (TIF urls have coords in UTM values)
    let min_coord_utm = lonlat_to_utm(input_bbox_envelope.MinX, input_bbox_envelope.MinY).unwrap();
    let max_coord_utm = lonlat_to_utm(input_bbox_envelope.MaxX, input_bbox_envelope.MaxY).unwrap();
    // scale UTM coordinates to match TIF URL format
    let scaled_min_x = min_coord_utm.0 / 10000.0;
    let scaled_max_x = max_coord_utm.0 / 10000.0;
    let scaled_min_y = min_coord_utm.1 / 100000.0;
    let scaled_max_y = max_coord_utm.1 / 100000.0;

    let mut overlapping_tif_urls = Vec::new();

    for tif_url in tif_urls {
        let coords_from_url =
            extract_coords_from_url(&tif_url).expect("Failed to extract coordinates from TIF URL");
        let tile_min_x = coords_from_url.0;
        let tile_min_y = coords_from_url.1 - 0.1;
        let tile_max_x = tile_min_x + 1.0;
        let tile_max_y = coords_from_url.1;

        let overlap_in_x = scaled_min_x <= tile_max_x && scaled_max_x >= tile_min_x;
        let overlap_in_y = scaled_min_y <= tile_max_y && scaled_max_y >= tile_min_y;

        if overlap_in_x && overlap_in_y {
            overlapping_tif_urls.push(tif_url.clone());
        }
    }

    overlapping_tif_urls
}
