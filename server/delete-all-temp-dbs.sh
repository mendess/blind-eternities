#!/bin/bash

./manage_db.sh s -c '\l' |
    grep -oE '[0-9a-z]{8}-[0-9a-z]{4}-[0-9a-z]{4}-[0-9a-z]{4}-[0-9a-z]{12}' |
    xargs -I{} ./manage_db.sh s -c 'drop database "{}"'
