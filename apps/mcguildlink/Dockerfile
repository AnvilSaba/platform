FROM eclipse-temurin:25-jdk AS builder
WORKDIR /workspace
COPY apps/mcguildlink/ ./
RUN ./gradlew --no-daemon installDist

FROM eclipse-temurin:25-jre
WORKDIR /app
COPY --from=builder /workspace/app/build/install/app/ /app/
RUN mkdir -p /app/config /app/data /app/static
ENTRYPOINT ["/app/bin/app"]