# gpkg-usgs-lookup

Server for seraching for USGS TIF download URL's given a WKT polygon to search for intersections with.

## testing

1. `docker run build . -t test-usgs-lookup`
2. `docker run -p 8080:8080 -it test-usgs-lookup`
3. [Open in your browser.](http://0.0.0.0:8080/1m-product-urls?bbox=POLYGON%20((-104.968487%2039.73642,-104.964238%2039.73642,-104.964238%2039.729283,-104.968487%2039.729283,-104.968487%2039.73642)))

## todo
- [ ] Fetch USGS metadata as part of docker build
- [ ] Search more than just the 1m res set of products