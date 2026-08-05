#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <linux/videodev2.h>
#include <poll.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

#define CP0_JPEG_DEVICE "/dev/video31"
#define CP0_JPEG_TIMEOUT_MS 1500

struct mapped_buffer {
    void *address;
    size_t length;
};

static int xioctl(int fd, unsigned long request, void *argument) {
    int result;
    do {
        result = ioctl(fd, request, argument);
    } while (result < 0 && errno == EINTR);
    return result;
}

static int set_format(int fd, enum v4l2_buf_type type, uint32_t fourcc,
                      uint32_t width, uint32_t height, uint32_t sizeimage,
                      struct v4l2_format *format) {
    memset(format, 0, sizeof(*format));
    format->type = type;
    format->fmt.pix_mp.width = width;
    format->fmt.pix_mp.height = height;
    format->fmt.pix_mp.pixelformat = fourcc;
    format->fmt.pix_mp.field = V4L2_FIELD_NONE;
    format->fmt.pix_mp.colorspace = V4L2_COLORSPACE_JPEG;
    format->fmt.pix_mp.ycbcr_enc = V4L2_YCBCR_ENC_DEFAULT;
    format->fmt.pix_mp.quantization = V4L2_QUANTIZATION_FULL_RANGE;
    format->fmt.pix_mp.num_planes = 1;
    format->fmt.pix_mp.plane_fmt[0].sizeimage = sizeimage;
    if (xioctl(fd, VIDIOC_S_FMT, format) < 0)
        return -errno;
    if (format->fmt.pix_mp.width != width ||
        format->fmt.pix_mp.height != height ||
        format->fmt.pix_mp.pixelformat != fourcc ||
        format->fmt.pix_mp.num_planes != 1)
        return -ENOTSUP;
    return 0;
}

static int map_one_buffer(int fd, enum v4l2_buf_type type,
                          struct mapped_buffer *mapped) {
    struct v4l2_requestbuffers request = {0};
    request.count = 1;
    request.type = type;
    request.memory = V4L2_MEMORY_MMAP;
    if (xioctl(fd, VIDIOC_REQBUFS, &request) < 0)
        return -errno;
    if (request.count < 1)
        return -ENOMEM;

    struct v4l2_plane plane = {0};
    struct v4l2_buffer buffer = {0};
    buffer.type = type;
    buffer.memory = V4L2_MEMORY_MMAP;
    buffer.index = 0;
    buffer.length = 1;
    buffer.m.planes = &plane;
    if (xioctl(fd, VIDIOC_QUERYBUF, &buffer) < 0)
        return -errno;
    mapped->length = plane.length;
    mapped->address = mmap(NULL, mapped->length, PROT_READ | PROT_WRITE,
                           MAP_SHARED, fd, plane.m.mem_offset);
    if (mapped->address == MAP_FAILED) {
        mapped->address = NULL;
        return -errno;
    }
    return 0;
}

static int queue_buffer(int fd, enum v4l2_buf_type type, uint32_t bytesused) {
    struct v4l2_plane plane = {0};
    struct v4l2_buffer buffer = {0};
    buffer.type = type;
    buffer.memory = V4L2_MEMORY_MMAP;
    buffer.index = 0;
    buffer.length = 1;
    buffer.m.planes = &plane;
    plane.bytesused = bytesused;
    if (xioctl(fd, VIDIOC_QBUF, &buffer) < 0)
        return -errno;
    return 0;
}

static int dequeue_capture(int fd, size_t *bytesused) {
    struct pollfd descriptor = {
        .fd = fd,
        .events = POLLIN | POLLPRI,
    };
    int ready;
    do {
        ready = poll(&descriptor, 1, CP0_JPEG_TIMEOUT_MS);
    } while (ready < 0 && errno == EINTR);
    if (ready == 0)
        return -ETIMEDOUT;
    if (ready < 0)
        return -errno;
    if (descriptor.revents & (POLLERR | POLLHUP | POLLNVAL))
        return -EIO;

    struct v4l2_plane plane = {0};
    struct v4l2_buffer buffer = {0};
    buffer.type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
    buffer.memory = V4L2_MEMORY_MMAP;
    buffer.length = 1;
    buffer.m.planes = &plane;
    if (xioctl(fd, VIDIOC_DQBUF, &buffer) < 0)
        return -errno;
    if (buffer.flags & V4L2_BUF_FLAG_ERROR)
        return -EIO;
    *bytesused = plane.bytesused;
    return 0;
}

int cp0_v4l2_encode_jpeg(const uint8_t *yuv420, size_t yuv420_length,
                         uint32_t width, uint32_t height, uint32_t quality,
                         uint8_t *jpeg, size_t jpeg_capacity,
                         size_t *jpeg_length) {
    if (yuv420 == NULL || jpeg == NULL || jpeg_length == NULL ||
        width == 0 || height == 0 || quality == 0 || quality > 100 ||
        yuv420_length != (size_t)width * height * 3 / 2 ||
        jpeg_capacity < 4)
        return -EINVAL;

    int result = 0;
    int fd = -1;
    bool output_streaming = false;
    bool capture_streaming = false;
    struct mapped_buffer output = {0};
    struct mapped_buffer capture = {0};
    struct v4l2_format output_format;
    struct v4l2_format capture_format;

    fd = open(CP0_JPEG_DEVICE, O_RDWR | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        result = -errno;
        goto done;
    }
    result = set_format(fd, V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
                        V4L2_PIX_FMT_YUV420, width, height,
                        (uint32_t)yuv420_length, &output_format);
    if (result < 0)
        goto done;
    result = set_format(fd, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
                        V4L2_PIX_FMT_JPEG, width, height,
                        (uint32_t)jpeg_capacity, &capture_format);
    if (result < 0)
        goto done;
    if (output_format.fmt.pix_mp.plane_fmt[0].bytesperline != width ||
        output_format.fmt.pix_mp.plane_fmt[0].sizeimage < yuv420_length) {
        result = -ENOTSUP;
        goto done;
    }

    struct v4l2_control control = {
        .id = V4L2_CID_JPEG_COMPRESSION_QUALITY,
        .value = (int32_t)quality,
    };
    if (xioctl(fd, VIDIOC_S_CTRL, &control) < 0 && errno != EINVAL) {
        result = -errno;
        goto done;
    }

    result = map_one_buffer(fd, V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE, &output);
    if (result < 0)
        goto done;
    result = map_one_buffer(fd, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, &capture);
    if (result < 0)
        goto done;
    if (output.length < yuv420_length || capture.length > jpeg_capacity) {
        result = -ENOSPC;
        goto done;
    }
    memcpy(output.address, yuv420, yuv420_length);

    result = queue_buffer(fd, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE, 0);
    if (result < 0)
        goto done;
    result = queue_buffer(fd, V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
                          (uint32_t)yuv420_length);
    if (result < 0)
        goto done;

    enum v4l2_buf_type output_type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
    enum v4l2_buf_type capture_type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
    if (xioctl(fd, VIDIOC_STREAMON, &output_type) < 0) {
        result = -errno;
        goto done;
    }
    output_streaming = true;
    if (xioctl(fd, VIDIOC_STREAMON, &capture_type) < 0) {
        result = -errno;
        goto done;
    }
    capture_streaming = true;

    size_t encoded_length = 0;
    result = dequeue_capture(fd, &encoded_length);
    if (result < 0)
        goto done;
    if (encoded_length < 4 || encoded_length > capture.length ||
        encoded_length > jpeg_capacity) {
        result = -EIO;
        goto done;
    }
    memcpy(jpeg, capture.address, encoded_length);
    if (jpeg[0] != 0xff || jpeg[1] != 0xd8 ||
        jpeg[encoded_length - 2] != 0xff || jpeg[encoded_length - 1] != 0xd9) {
        result = -EIO;
        goto done;
    }
    *jpeg_length = encoded_length;

done:
    if (fd >= 0) {
        enum v4l2_buf_type type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
        if (capture_streaming)
            (void)xioctl(fd, VIDIOC_STREAMOFF, &type);
        type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
        if (output_streaming)
            (void)xioctl(fd, VIDIOC_STREAMOFF, &type);
    }
    if (capture.address != NULL)
        (void)munmap(capture.address, capture.length);
    if (output.address != NULL)
        (void)munmap(output.address, output.length);
    if (fd >= 0)
        (void)close(fd);
    return result;
}
