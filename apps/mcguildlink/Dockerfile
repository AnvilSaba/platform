FROM eclipse-temurin:25-jdk AS builder
WORKDIR /workspace
COPY apps/mcguildlink/ ./
RUN ./gradlew --no-daemon --no-parallel --no-configuration-cache installDist generateLicenseReport

FROM eclipse-temurin:25-jre
WORKDIR /app
COPY --from=builder /workspace/app/build/install/app/ /app/
COPY LICENSE /app/LICENSE
COPY --from=builder /workspace/app/build/reports/dependency-license/ /app/THIRD_PARTY_LICENSES/
RUN mkdir -p /app/config /app/data /app/static
ENTRYPOINT ["/app/bin/app"]
