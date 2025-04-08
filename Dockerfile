# start from gdal image (@ specifc version that works with rust's gdal-sys crate out of the box)
FROM ghcr.io/osgeo/gdal:ubuntu-full-3.9.1
# install rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# make sure cargo is in the path so we can build
ENV PATH="/root/.cargo/bin:${PATH}"    
# bonus helper packages
RUN apt update && \apt install -y build-essential libssl-dev git pkg-config libclang-dev

WORKDIR /usr/src/app
COPY . .
# Build with specific library paths and verbose output
RUN RUSTFLAGS="-L /usr/lib -L /lib" cargo build --release

CMD ["/usr/src/app/target/release/gpkg-usgs-lookup"]
