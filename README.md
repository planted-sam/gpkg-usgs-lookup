# gpkg-usgs-lookup

Server for seraching for USGS TIF download URL's given a WKT polygon to search for intersections with. The intent is that the consumer of this server would then take this returned set of TIF URI's and do some mosaic reading + trimming w/the input polygon to make the desired DEM from USGS survey data.

## running locally

1. Download dataset from USGS S3 bucket (~1.2GB), save as `FESM_1m.gpkg`, here's a CURL command you can run in this dir
```sh
curl -L -o FESM_1m.gpkg https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1m/FullExtentSpatialMetadata/FESM_1m.gpkg
```
2. (Ensure you have gdal/etc installed locally &) Run `cargo run dev`.
3. With the server running, use CURL/Postman/your browser to make a GET request to `0.0.0.0:8080/1m-product-urls` with a polygon WKT passed into the `bbox` query param. E.g.
```
http://0.0.0.0:8080/1m-product-urls?bbox=POLYGON ((-119.17782075628 35.7741183769118,-119.176440971049 35.7741183769118,-119.176440971049 35.775892139343,-119.17782075628 35.775892139343,-119.17782075628 35.7741183769118))
```
4. The expected response should be a text response of a list of found/intersecting tif urls, e.g. (for the polygon in th URL abaove)
```
["https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1m/Projects/CA_SanJoaquin_2021_A21/TIFF/USGS_1M_11_x30y397_CA_SanJoaquin_2021_A21.tif"]
```

## testing

1. `docker run build . -t test-usgs-lookup`
2. `docker run -p 8080:8080 -it test-usgs-lookup`
3. [Open in your browser.](http://0.0.0.0:8080/1m-product-urls?bbox=POLYGON%20((-104.968487%2039.73642,-104.964238%2039.73642,-104.964238%2039.729283,-104.968487%2039.729283,-104.968487%2039.73642)))

## todo
- [ ] Search more than just the 1m res set of products
- [ ] Prefer latest product set if multiple are available (or fetch all)
