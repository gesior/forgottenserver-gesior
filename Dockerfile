FROM alpine:3.19.0 AS build
ARG BUILD_CORES=1
RUN apk add --no-cache \
  binutils \
  boost-dev \
  build-base \
  clang \
  cmake \
  fmt-dev \
  gcc \
  gmp-dev \
  luajit-dev \
  make \
  mariadb-connector-c-dev \
  openssl-dev \
  pugixml-dev

COPY cmake /usr/src/forgottenserver/cmake/
COPY src /usr/src/forgottenserver/src/
COPY CMakeLists.txt /usr/src/forgottenserver/
WORKDIR /usr/src/forgottenserver/build
RUN cmake .. && make -j $BUILD_CORES

FROM alpine:3.19.0
RUN apk add --no-cache \
  boost-iostreams \
  boost-system \
  fmt \
  gmp \
  luajit \
  mariadb-connector-c \
  openssl-dev \
  pugixml

COPY --from=build /usr/src/forgottenserver/build/tfs /bin/tfs
COPY data /srv/data/
COPY LICENSE README.md *.dist *.sql key.pem /srv/

EXPOSE 7171 7172
WORKDIR /srv
VOLUME /srv
ENTRYPOINT ["/bin/tfs"]
