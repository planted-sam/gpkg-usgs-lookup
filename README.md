# gpkg-usgs-lookup
Simple program for searching local GeoPackage files for specific polygons meeting an intersection criteria (WIP)

## setup (WIP)

1. Ensure rust is installed
2. Ensure both `gdal` and `libgdal-dev` are installed on your system (working on Docker bear with me)
3. [Download metadata file](https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1m/FullExtentSpatialMetadata/FESM_1m.gpkg) for 1m resolution USGS data, save in this directory as `FESM_1m.gpkg`
4. Run `cargo run dev` to see example case run.

## todo

- [ ] Make it an HTTP server to hit with a bbox
- [ ] Make metadata requests faster
