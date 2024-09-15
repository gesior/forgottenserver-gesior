FROM ubuntu:24.04 AS build

ARG DEBIAN_FRONTEND=noninteractive
ENV TZ=Europe/London

RUN apt-get update && apt-get -yq install tzdata
RUN apt-get update && apt-get -yq install git cmake build-essential libluajit-5.1-dev \
    libmysqlclient-dev libboost-system-dev libboost-iostreams-dev libboost-filesystem-dev \
	libpugixml-dev libcrypto++-dev libfmt-dev libboost-date-time-dev git cmake build-essential \
	liblua5.4-dev libgmp3-dev libmysqlclient-dev libboost-locale-dev libboost-json-dev \
	libboost-system-dev libboost-iostreams-dev libboost-filesystem-dev libpugixml-dev libcrypto++-dev

COPY cmake /usr/src/forgottenserver/cmake/
COPY src /usr/src/forgottenserver/src/
COPY CMakeLists.txt /usr/src/forgottenserver/
WORKDIR /usr/src/forgottenserver/build
RUN cmake -DCMAKE_BUILD_TYPE=Release .. && make -j $(nproc)

FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ENV TZ=Europe/London

RUN apt-get update && apt-get -yq install tzdata
RUN apt-get update && apt-get -yq install git cmake build-essential libluajit-5.1-dev \
    libmysqlclient-dev libboost-system-dev libboost-iostreams-dev libboost-filesystem-dev \
	libpugixml-dev libcrypto++-dev libfmt-dev libboost-date-time-dev git cmake build-essential \
	liblua5.4-dev libgmp3-dev libmysqlclient-dev libboost-locale-dev libboost-json-dev \
	libboost-system-dev libboost-iostreams-dev libboost-filesystem-dev libpugixml-dev libcrypto++-dev

COPY --from=build /usr/src/forgottenserver/build/tfs /bin/tfs

COPY data /srv/data/
COPY LICENSE README.md *.dist *.sql key.pem /srv/

EXPOSE 7171 7172
WORKDIR /srv
VOLUME /srv
ENTRYPOINT ["/bin/tfs"]
