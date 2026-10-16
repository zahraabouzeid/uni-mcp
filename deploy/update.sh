#!/bin/sh
set -eu
cd "$(dirname "$0")"

container=$(docker compose ps --all --quiet app)
docker compose pull --quiet
latest=$(docker image inspect --format '{{.Id}}' "$(docker compose config --images | grep '/unimcp:')")

if [ -n "$container" ] && [ "$(docker inspect --format '{{.Image}}' "$container")" != "$latest" ]; then
	mkdir -p backups
	chmod 700 backups
	archive="unimcp-$(date +%Y-%m-%d-%H%M).tar.gz"
	docker compose stop app
	docker run --rm --volumes-from "$container" --volume "$PWD/backups:/backups" busybox \
		sh -c "tar czf /backups/$archive -C /data . && chown $(id -u):$(id -g) /backups/$archive"
	ls -1t backups/unimcp-*.tar.gz | tail -n +8 | xargs -r rm --
fi

docker compose up --detach
docker image prune --force
