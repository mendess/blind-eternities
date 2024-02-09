FROM prom/prometheus
COPY prometheus.yml /etc/prometheus/prometheus.yml


global:
  scrape_interval:     5s
  evaluation_interval: 5s

scrape_configs:
  - job_name: 'prometheus'
    static_configs:
      - targets: ['localhost:9090']

  - job_name: 'service-collector'
    static_configs:
      - targets: ['localhost:8080']
