/*
 * CardputerZero V0.6 pre-driver splash renderer.
 *
 * The register-level ST7789 bring-up is adapted from CardputerZero/pi-gen
 * commit e05b81c80f1f5a8e589956937adba5b5d04f0ca9. Unlike that experimental
 * implementation, this is a bounded initramfs helper rather than PID 1.
 */

#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#ifndef O_NOFOLLOW
#define O_NOFOLLOW 0
#endif

#define BCM2837_PERIPHERAL_BASE 0x3f000000UL
#define GPIO_BASE (BCM2837_PERIPHERAL_BASE + 0x00200000UL)
#define SPI0_BASE (BCM2837_PERIPHERAL_BASE + 0x00204000UL)
#define REGISTER_MAP_BYTES 4096UL

#define GPFSEL0 0x00U
#define GPSET0 0x1cU
#define GPCLR0 0x28U

#define SPI_CS 0x00U
#define SPI_FIFO 0x04U
#define SPI_CLK 0x08U

#define SPI_CS_TA (1U << 7)
#define SPI_CS_DONE (1U << 16)
#define SPI_CS_RXD (1U << 17)
#define SPI_CS_TXD (1U << 18)
#define SPI_CS_CLEAR_RX (1U << 5)
#define SPI_CS_CLEAR_TX (1U << 4)

#define PIN_DC 25U
#define DISPLAY_WIDTH 320U
#define DISPLAY_HEIGHT 170U
#define DISPLAY_Y_OFFSET 35U
#define SPLASH_BYTES (DISPLAY_WIDTH * DISPLAY_HEIGHT * 2U)
#define SPI_WAIT_LIMIT 20000000U
#define RENDER_TIMEOUT_SECONDS 2U

#define ST7789_SLPOUT 0x11U
#define ST7789_DISPON 0x29U
#define ST7789_CASET 0x2aU
#define ST7789_RASET 0x2bU
#define ST7789_RAMWR 0x2cU
#define ST7789_MADCTL 0x36U
#define ST7789_COLMOD 0x3aU

static volatile uint32_t *gpio_registers;
static volatile uint32_t *spi_registers;
static uint8_t splash[SPLASH_BYTES];

static volatile uint32_t *reg(volatile uint32_t *base, uint32_t offset)
{
    return &base[offset / sizeof(uint32_t)];
}

static void gpio_set(uint32_t pin)
{
    *reg(gpio_registers, GPSET0) = 1U << pin;
}

static void gpio_clear(uint32_t pin)
{
    *reg(gpio_registers, GPCLR0) = 1U << pin;
}

static void gpio_function(uint32_t pin, uint32_t function)
{
    uint32_t offset = GPFSEL0 + (pin / 10U) * sizeof(uint32_t);
    uint32_t shift = (pin % 10U) * 3U;
    uint32_t value = *reg(gpio_registers, offset);

    value &= ~(7U << shift);
    value |= function << shift;
    *reg(gpio_registers, offset) = value;
}

static int wait_for_spi(uint32_t mask)
{
    uint32_t attempts;

    for (attempts = 0; attempts < SPI_WAIT_LIMIT; attempts++) {
        if ((*reg(spi_registers, SPI_CS) & mask) != 0U)
            return 0;
    }
    return -1;
}

static int drain_receive_fifo(void)
{
    uint32_t attempts;

    for (attempts = 0; attempts < SPI_WAIT_LIMIT; attempts++) {
        if ((*reg(spi_registers, SPI_CS) & SPI_CS_RXD) == 0U)
            return 0;
        (void)*reg(spi_registers, SPI_FIFO);
    }
    return -1;
}

static int spi_transfer(const uint8_t *bytes, size_t length, int swap_pairs)
{
    size_t index;

    *reg(spi_registers, SPI_CS) =
        SPI_CS_TA | SPI_CS_CLEAR_TX | SPI_CS_CLEAR_RX;
    for (index = 0; index < length; index++) {
        size_t source = swap_pairs ? (index ^ 1U) : index;

        if (wait_for_spi(SPI_CS_TXD) != 0)
            goto fail;
        *reg(spi_registers, SPI_FIFO) = bytes[source];
        if (drain_receive_fifo() != 0)
            goto fail;
    }
    if (wait_for_spi(SPI_CS_DONE) != 0)
        goto fail;
    if (drain_receive_fifo() != 0)
        goto fail;
    *reg(spi_registers, SPI_CS) = 0U;
    return 0;

fail:
    *reg(spi_registers, SPI_CS) = 0U;
    return -1;
}

static int command(uint8_t value)
{
    gpio_clear(PIN_DC);
    return spi_transfer(&value, 1U, 0);
}

static int data(const uint8_t *bytes, size_t length, int swap_pairs)
{
    gpio_set(PIN_DC);
    return spi_transfer(bytes, length, swap_pairs);
}

static int command_data(uint8_t command_value, const uint8_t *bytes,
                        size_t length)
{
    return command(command_value) == 0 && data(bytes, length, 0) == 0 ? 0 : -1;
}

static int delay_milliseconds(long milliseconds)
{
    struct timespec delay = {
        .tv_sec = milliseconds / 1000L,
        .tv_nsec = (milliseconds % 1000L) * 1000000L,
    };

    while (nanosleep(&delay, &delay) != 0) {
        if (errno != EINTR)
            return -1;
    }
    return 0;
}

static int load_splash(const char *path)
{
    struct stat metadata;
    size_t offset = 0;
    int fd;

    fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0)
        return -1;
    if (fstat(fd, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_size != (off_t)sizeof(splash)) {
        close(fd);
        return -1;
    }
    while (offset < sizeof(splash)) {
        ssize_t count = read(fd, splash + offset, sizeof(splash) - offset);

        if (count < 0 && errno == EINTR)
            continue;
        if (count <= 0) {
            close(fd);
            return -1;
        }
        offset += (size_t)count;
    }
    if (close(fd) != 0)
        return -1;
    return 0;
}

static void initialize_spi(void)
{
    /* BCM283x GPIO ALT0 selects CE0, MISO, MOSI and SCLK on GPIO 8-11. */
    gpio_function(8U, 4U);
    gpio_function(9U, 4U);
    gpio_function(10U, 4U);
    gpio_function(11U, 4U);
    gpio_function(PIN_DC, 1U);
    gpio_set(PIN_DC);

    /* 250 MHz / 12 is close to the V0.6 driver's validated 20 MHz limit. */
    *reg(spi_registers, SPI_CLK) = 12U;
    *reg(spi_registers, SPI_CS) = SPI_CS_CLEAR_TX | SPI_CS_CLEAR_RX;
}

static int initialize_display(void)
{
    const uint8_t madctl = 0x60U;
    const uint8_t color_mode = 0x55U;

    if (command(ST7789_SLPOUT) != 0 || delay_milliseconds(120L) != 0 ||
        command_data(ST7789_MADCTL, &madctl, 1U) != 0 ||
        command_data(ST7789_COLMOD, &color_mode, 1U) != 0 ||
        command(ST7789_DISPON) != 0 || delay_milliseconds(20L) != 0)
        return -1;
    return 0;
}

static int set_display_window(void)
{
    const uint16_t x1 = DISPLAY_WIDTH - 1U;
    const uint16_t y0 = DISPLAY_Y_OFFSET;
    const uint16_t y1 = DISPLAY_Y_OFFSET + DISPLAY_HEIGHT - 1U;
    const uint8_t columns[] = {0U, 0U, (uint8_t)(x1 >> 8), (uint8_t)x1};
    const uint8_t rows[] = {
        (uint8_t)(y0 >> 8), (uint8_t)y0, (uint8_t)(y1 >> 8), (uint8_t)y1,
    };

    if (command_data(ST7789_CASET, columns, sizeof(columns)) != 0 ||
        command_data(ST7789_RASET, rows, sizeof(rows)) != 0 ||
        command(ST7789_RAMWR) != 0)
        return -1;
    return 0;
}

static void *map_registers(int fd, off_t address)
{
    return mmap(NULL, REGISTER_MAP_BYTES, PROT_READ | PROT_WRITE, MAP_SHARED,
                fd, address);
}

static int render_splash(void)
{
    int fd;
    int result = -1;

    fd = open("/dev/mem", O_RDWR | O_SYNC | O_CLOEXEC);
    if (fd < 0)
        return -1;
    gpio_registers = map_registers(fd, (off_t)GPIO_BASE);
    spi_registers = map_registers(fd, (off_t)SPI0_BASE);
    close(fd);
    if (gpio_registers == MAP_FAILED || spi_registers == MAP_FAILED)
        goto unmap;

    initialize_spi();
    if (initialize_display() == 0 && set_display_window() == 0) {
        /* The Linux fbdev asset is little-endian; ST7789 takes MSB first. */
        result = data(splash, sizeof(splash), 1);
    }

unmap:
    if (gpio_registers != MAP_FAILED)
        munmap((void *)gpio_registers, REGISTER_MAP_BYTES);
    if (spi_registers != MAP_FAILED)
        munmap((void *)spi_registers, REGISTER_MAP_BYTES);
    return result;
}

int main(int argc, char **argv)
{
    int check_only = 0;
    const char *path;

    if (argc == 3 && strcmp(argv[1], "--check-image") == 0) {
        check_only = 1;
        path = argv[2];
    } else if (argc == 2) {
        path = argv[1];
    } else {
        return EXIT_FAILURE;
    }
    if (load_splash(path) != 0)
        return EXIT_FAILURE;
    if (check_only)
        return EXIT_SUCCESS;
    (void)alarm(RENDER_TIMEOUT_SECONDS);
    int result = render_splash();
    (void)alarm(0U);
    return result == 0 ? EXIT_SUCCESS : EXIT_FAILURE;
}
