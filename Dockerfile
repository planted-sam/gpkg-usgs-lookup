# start from gdal image (@ specifc version that works with rust's gdal-sys crate out of the box)
FROM ghcr.io/osgeo/gdal:ubuntu-full-3.9.1
# install rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# make sure cargo is in the path so we can build
ENV PATH="/root/.cargo/bin:${PATH}"    
# bonus helper packages
RUN apt update && \apt install -y build-essential libssl-dev git pkg-config libclang-dev libproj-dev
# env setup done, prepare app code for build
WORKDIR /usr/src/app
# download the .gpkg file for 1m resolution USGS products (~1.2GB give it a sec)
RUN curl -L -o FESM_1m.gpkg https://prd-tnm.s3.amazonaws.com/StagedProducts/Elevation/1m/FullExtentSpatialMetadata/FESM_1m.gpkg
# copy app code
COPY . .
# build
RUN RUSTFLAGS="-L /usr/lib -L /lib" cargo build --release
# run it!
CMD ["/usr/src/app/target/release/gpkg-usgs-lookup"]
