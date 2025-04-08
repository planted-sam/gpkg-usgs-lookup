use axum::Error;
use axum::{Router, response::IntoResponse, routing::get, extract::Query};
use futures::future::join_all;
use gdal::{
    Dataset,
    vector::{Geometry, LayerAccess},
};
use quick_xml::{events::Event, reader::Reader};
use std::path::Path;
use serde::Deserialize;

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

        let only_overlapping_links = find_overlapping_files(&download_links, &params.bbox)
            .await
            .expect("Failed to filter down complete set fo tifs to jsut overlapping ones.");
        println!("Overlapping ones: {:?}", only_overlapping_links);
        return_str =format!("{:?}", only_overlapping_links);
    }
    return_str
}

struct USGSProductResult {
    pub product_link: String,
    pub metadata_link: String,
    pub date: String,
    pub name: String,
}

struct TIFBBox {
    westbc: f64,
    eastbc: f64,
    northbc: f64,
    southbc: f64,
}

// searches 1m metadata db for overlapping projects
fn search_gpkg_dataset(bbox_wkt: &String) -> gdal::errors::Result<Vec<USGSProductResult>> {
    let bbox = Geometry::from_wkt(bbox_wkt).unwrap();
    let mut output = vec![];
    let dataset = Dataset::open(Path::new("FESM_1m.gpkg")).unwrap();
    for mut layer in dataset.layers() {
        for feature in layer.features() {
            let feature_geom = feature.geometry();
            match feature_geom {
                Some(outer_geom) => {
                    let inner_geom_count = outer_geom.geometry_count();
                    if inner_geom_count > 0 {
                        for inner_geom_idx in 0..inner_geom_count {
                            let inner_geom = outer_geom.get_geometry(inner_geom_idx);
                            if inner_geom.intersects(&bbox) {
                                output.push(USGSProductResult {
                                    product_link: feature
                                        .field_as_string(
                                            feature.field_index("product_link").unwrap(),
                                        )
                                        .unwrap()
                                        .unwrap(),
                                    metadata_link: feature
                                        .field_as_string(
                                            feature.field_index("metadata_link").unwrap(),
                                        )
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
                                })
                            }
                        }
                    }
                }
                None => {}
            }
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

fn get_xml_url_from_tif_url(tif_url: &String) -> String {
    tif_url
        .replace("/TIFF/", "/metadata/")
        .replace(".tif", ".xml")
}

async fn find_overlapping_files(tif_urls: &[String], input_bbox_wkt: &String) -> Result<Vec<String>, reqwest::Error> {
    let tasks: Vec<_> = tif_urls.iter().map(|tif_url| {
        let tif_url = tif_url.clone();
        let bbox_wkt = input_bbox_wkt.clone();

        tokio::spawn(async move {
            let xml_url = get_xml_url_from_tif_url(&tif_url);
            let response = reqwest::get(xml_url).await.expect("Failed to fetch XML");
            let xml_str = response.text().await.expect("Failed to parse XML");

            let bbox_info = parse_xml_for_bbox(&xml_str).unwrap();

            let tiff_bbox_geom = Geometry::bbox(bbox_info.westbc, bbox_info.southbc, bbox_info.eastbc, bbox_info.northbc).unwrap();
            let input_bbox = Geometry::from_wkt(&bbox_wkt).unwrap();

            if tiff_bbox_geom.intersects(&input_bbox) {
                Ok::<Option<String>, Error>(Some(tif_url))
            } else {
                Ok(None)
            }
        })
    }).collect();

    let results = join_all(tasks).await;
    let mut overlapping_tif_urls = Vec::new();

    for result in results {
        match result {
            Ok(inner_result) => match inner_result.expect("Failed to process TIF URL") {
                Some(url) => overlapping_tif_urls.push(url),
                None => {}
            },
            Err(_) => {}
        }
    }

    Ok(overlapping_tif_urls)
}

fn parse_xml_for_bbox(xml_str: &str) -> Result<TIFBBox, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_str(xml_str);
    let mut bbox_info = TIFBBox { westbc: 0.0, eastbc: 0.0, northbc: 0.0, southbc: 0.0 };
    let mut tag = None;
    loop {
        match reader.read_event()? {
            Event::Start(ref e) => match e.name().0 {
                b"westbc" => tag = Some("westbc"),
                b"eastbc" => tag = Some("eastbc"),
                b"northbc" => tag = Some("northbc"),
                b"southbc" => tag = Some("southbc"),
                _ => tag = None,
            },
            Event::Text(e) if tag.is_some() => {
                let value: f64 = e.unescape()?.parse()?;
                match tag.unwrap() {
                    "westbc" => bbox_info.westbc = value,
                    "eastbc" => bbox_info.eastbc = value,
                    "northbc" => bbox_info.northbc = value,
                    "southbc" => bbox_info.southbc = value,
                    _ => (),
                }
            }
            Event::End(_) => tag = None,
            Event::Eof => break,
            _ => (),
        }
    }
    Ok(bbox_info)
}
