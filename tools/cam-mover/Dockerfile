FROM php:8.2-cli-alpine

RUN apk update && \
    (apk add --no-cache 7zip || apk add --no-cache p7zip) && \
    docker-php-ext-install pcntl pdo_mysql

WORKDIR /app

COPY config.php mover.php ./

ENV CAMS_SOURCE_DIR=/srv/cams/
ENV CAMS_DEST_DIR=/srv/camsReady/
ENV DB_HOST=127.0.0.1
ENV DB_PORT=3306
ENV DB_NAME=forgottenserver
ENV DB_USER=forgottenserver
ENV DB_PASS=
ENV SEVENZIP_PATH=7z

CMD ["php", "mover.php"]
