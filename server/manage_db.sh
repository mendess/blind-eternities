#!/bin/bash

CONTAINER_NAME=blind-eternities-db
DB=blind_eternities
if [[ "$T" ]]; then
    DB=${DB}_test
fi

if ! pgrep docker >/dev/null; then
    echo "Start docker"
    sudo systemctl start docker
fi

machine_ip() {
    IP=$(docker inspect "$CONTAINER_NAME" |
        jq '.[0].NetworkSettings.Networks.bridge.IPAddress' -r)
    echo "$IP"
}

check_run() {
    if docker ps | grep -q "$CONTAINER_NAME"; then
        :
    elif docker ps -a | grep -q "$CONTAINER_NAME"; then
        docker start "$CONTAINER_NAME"
    else
        docker run \
            --name "$CONTAINER_NAME" \
            -e POSTGRES_PASSWORD=postgres \
            -d \
            -p 5432:5432 \
            postgres \
            -c log_statement=all
    fi
}

main() {
    case "$1" in
        s | shell)
            check_run
            if command -V psql &>/dev/null; then
                ip="$(machine_ip)"
                PAGER="nvim -R -c 'syntax on' -c 'set syntax=dbout' -" \
                    PGPASSWORD=postgres \
                    psql -U postgres -d "$DB" -h "$ip" "${@:2}"
            else
                docker exec -it "$CONTAINER_NAME" \
                    psql -U postgres -d "$DB" "${@:2}"
            fi
            ;;
        r | run)
            args=()
            while read -r arg; do
                args+=("$arg")
            done < <(grep "$2" .manage_db.saves | cut -f2- -d, | sed 's/:::/\n/g')

            main s "${args[@]}"
            ;;
        save)
            [[ "$#" -gt 2 ]] && [[ "$2" != -* ]] && {
                printf '%s,' "$2"
                shift 2
                printf "%s:::" "$@"
                echo
            } | sed s/:::$//g >>.manage_db.saves
            ;;
        c | create)
            check_run
            docker exec -it "$CONTAINER_NAME" \
                psql -U postgres -c "create database $DB" \
                -c "create database ${DB}_test"
            ;;
        ip)
            check_run
            machine_ip
            ;;
        stop)
            docker stop "$CONTAINER_NAME"
            ;;
        *)
            check_run
            echo "machine ip: $(machine_ip)"
            ;;
    esac

}
main "$@"
