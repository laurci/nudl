image := "ghcr.io/laurci/nudl"
tag := "alpha"

# Build and push the Docker image for both arm64 and amd64
docker-release:
    docker buildx build --platform linux/amd64 -t {{ image }}:{{ tag }} --push .
    # Temporarily disable arm64 build
    # docker buildx build --platform linux/arm64 -t {{ image }}:{{ tag }} --push .

# Build the Docker image for the current platform only (for local testing)
docker-build-local:
    docker buildx build -t {{ image }}:{{ tag }} --load .
