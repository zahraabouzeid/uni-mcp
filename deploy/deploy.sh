#!/bin/sh
set -eu
cd "$(dirname "$0")"
service=${1:-app}
if [ "$service" = app-dev ]; then
	export COMPOSE_PROFILES=dev
fi
proxy=$(docker compose config --services | grep -x caddy || true)
if [ -z "$proxy" ]; then
	docker network inspect web > /dev/null 2>&1 || docker network create web
fi

container=$(docker compose ps --all --quiet "$service")
docker compose pull --quiet "$service" $proxy
latest=$(docker image inspect --format '{{.Id}}' "$(docker compose config --images "$service")")

if [ -n "$container" ] && [ "$(docker inspect --format '{{.Image}}' "$container")" != "$latest" ]; then
	mkdir -p backups
	chmod 700 backups
	archive="$service-$(date +%Y-%m-%d-%H%M).tar.gz"
	docker compose stop "$service"
	docker run --rm --volumes-from "$container" --volume "$PWD/backups:/backups" busybox \
		sh -c "tar czf /backups/$archive -C /data . && chown $(id -u):$(id -g) /backups/$archive"
	ls -1t "backups/$service"-[0-9]*.tar.gz | tail -n +8 | xargs -r rm --
fi

docker compose up --detach "$service" $proxy
if [ -n "$proxy" ]; then
	docker compose exec caddy caddy reload --config /etc/caddy/Caddyfile
fi
docker image prune --force
