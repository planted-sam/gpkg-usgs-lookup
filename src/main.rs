use futures::future::join_all;
use gdal::{
    Dataset,
    vector::{Geometry, LayerAccess},
};
use quick_xml::{events::Event, reader::Reader};
use std::path::Path;

#[derive(Debug)]
struct USGSProductResult {
    pub product_link: String,
    pub metadata_link: String,
    pub date: String,
    pub name: String,
}

#[derive(Debug)]
struct TIFBBox {
    westbc: f64,
    eastbc: f64,
    northbc: f64,
    southbc: f64,
}

// searches 1m metadata db for overlapping projects
fn search_gpkg_dataset(bbox: &Geometry) -> gdal::errors::Result<Vec<USGSProductResult>> {
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

// Used to filter down our list of downlaod files to just ones that matter for our qeuried area
async fn find_overlapping_files(
    tif_urls: &[String], 
    input_bbox: &Geometry,
) -> Result<Vec<String>, reqwest::Error> {
    let mut tasks = Vec::new();
    // convert geom to WKT (string format essentially) because we can't share a Geometry between futures
    let input_bbox_wkt = input_bbox.wkt().unwrap();
    for tif_url in tif_urls.iter() {
        let input_bbox_wkt = input_bbox_wkt.clone();
        let tif_url = tif_url.clone();
        // Spawn a new async task for each TIF URL
        let task = tokio::spawn(async move {
            let xml_url = get_xml_url_from_tif_url(&tif_url);
            let response = reqwest::get(xml_url)
                .await
                .expect("Failed to fetch XML metadata");
            let xml_str = response.text().await.expect("Failed to parse XML metadata");

            let mut reader = Reader::from_str(&xml_str);
            let mut bbox_info = TIFBBox {
                westbc: 0.0,
                eastbc: 0.0,
                northbc: 0.0,
                southbc: 0.0,
            };

            let mut tag = None;
            loop {
                match reader.read_event() {
                    Ok(Event::Start(ref e)) => match e.name().0 {
                        b"westbc" => tag = Some("westbc"),
                        b"eastbc" => tag = Some("eastbc"),
                        b"northbc" => tag = Some("northbc"),
                        b"southbc" => tag = Some("southbc"),
                        _ => tag = None,
                    },
                    Ok(Event::Text(e)) => {
                        if let Some(ref t) = tag {
                            let value = e.unescape().unwrap().into_owned().parse::<f64>().unwrap();
                            match *t {
                                "westbc" => bbox_info.westbc = value,
                                "eastbc" => bbox_info.eastbc = value,
                                "northbc" => bbox_info.northbc = value,
                                "southbc" => bbox_info.southbc = value,
                                _ => (),
                            }
                        }
                    }
                    Ok(Event::End(_)) => tag = None,
                    Ok(Event::Eof) => break,
                    Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
                    _ => (),
                }
            }

            let tiff_bbox_geom = Geometry::bbox(
                bbox_info.westbc,
                bbox_info.southbc,
                bbox_info.eastbc,
                bbox_info.northbc,
            )
            .unwrap();
            let input_bbox = Geometry::from_wkt(input_bbox_wkt.as_str()).unwrap();
            if tiff_bbox_geom.intersects(&input_bbox) {
                Some(tif_url)
            } else {
                None
            }
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete
    let results = join_all(tasks).await;
    let mut overlapping_tif_urls = Vec::new();

    for result in results {
        if let Ok(Some(url)) = result {
            overlapping_tif_urls.push(url);
        }
    }

    Ok(overlapping_tif_urls)
}

#[tokio::main]
async fn main() {
    // test bbox of cheeseman park
    let bbox = Geometry::bbox(-104.968487, 39.729283, -104.964238, 39.736420).unwrap();
    let results = search_gpkg_dataset(&bbox).unwrap();
    if results.len() > 0 {
        // only check first result for now - TODO: pick most recent
        let download_links = download_list_of_download_links(&results[0])
            .await
            .expect("Failed to fetch download links for requested bbox.");
        println!("Successfully grabbed list of TIF's, checking to find overlap...");
        let only_overlapping_links = find_overlapping_files(&download_links, &bbox)
            .await
            .expect("Failed to filter down complete set fo tifs to jsut overlapping ones.");
        println!("Overlapping ones: {:?}", only_overlapping_links);
    }
}
