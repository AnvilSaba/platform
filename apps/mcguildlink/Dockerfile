FROM --platform=$BUILDPLATFORM eclipse-temurin:25-jdk AS builder
WORKDIR /workspace
COPY apps/mcguildlink/ ./
RUN ./gradlew --no-daemon --no-parallel --no-configuration-cache installDist generateLicenseReport && \
    mkdir -p /workspace/runtime/config /workspace/runtime/data /workspace/runtime/static

FROM gcr.io/distroless/java25-debian13:nonroot
WORKDIR /app
COPY --from=builder --chown=nonroot:nonroot /workspace/app/build/install/app/lib/ /app/lib/
COPY --chown=nonroot:nonroot LICENSE /app/LICENSE
COPY --from=builder --chown=nonroot:nonroot /workspace/app/build/reports/dependency-license/ /app/THIRD_PARTY_LICENSES/
COPY --from=builder --chown=nonroot:nonroot /workspace/runtime/ /app/
ENTRYPOINT ["/usr/bin/java", "-Dfile.encoding=UTF-8", "-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8", "-cp", "/app/lib/*", "io.github.anvilsaba.mcguildlink.app.AppKt"]
