#include <errno.h>
#include <sys/ioctl.h>
#include <termios.h>

/*
 * POSIX termios operations are expressed using the Linux ioctl ABI so the
 * runtime needs only the standard ioctl syscall capability, not a Node- or
 * terminal-shaped host import.
 * POSIX: https://pubs.opengroup.org/onlinepubs/9799919799/basedefs/termios.h.html
 * Linux ABI: https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/ioctls.h
 */
int tcgetattr(int fd, struct termios *value) {
    return ioctl(fd, TCGETS, value);
}

int tcsetattr(int fd, int action, const struct termios *value) {
    if (action < TCSANOW || action > TCSAFLUSH) {
        errno = EINVAL;
        return -1;
    }
    return ioctl(fd, TCSETS + action, value);
}

int tcdrain(int fd) { return ioctl(fd, TCSBRK, 1); }
int tcflow(int fd, int action) { return ioctl(fd, TCXONC, action); }
int tcflush(int fd, int queue) { return ioctl(fd, TCFLSH, queue); }
int tcsendbreak(int fd, int duration) {
    (void)duration;
    return ioctl(fd, TCSBRK, 0);
}

int tcgetwinsize(int fd, struct winsize *value) {
    return ioctl(fd, TIOCGWINSZ, value);
}

int tcsetwinsize(int fd, const struct winsize *value) {
    return ioctl(fd, TIOCSWINSZ, value);
}

pid_t tcgetsid(int fd) {
    int sid;
    return ioctl(fd, TIOCGSID, &sid) < 0 ? (pid_t)-1 : (pid_t)sid;
}

speed_t cfgetospeed(const struct termios *value) {
    return value->c_cflag & CBAUD;
}

speed_t cfgetispeed(const struct termios *value) {
    return cfgetospeed(value);
}

int cfsetospeed(struct termios *value, speed_t speed) {
    if (speed & ~CBAUD) {
        errno = EINVAL;
        return -1;
    }
    value->c_cflag = (value->c_cflag & ~CBAUD) | speed;
    return 0;
}

int cfsetispeed(struct termios *value, speed_t speed) {
    return speed ? cfsetospeed(value, speed) : 0;
}

int cfsetspeed(struct termios *value, speed_t speed) {
    return cfsetospeed(value, speed);
}

void cfmakeraw(struct termios *value) {
    value->c_iflag &= ~(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    value->c_oflag &= ~OPOST;
    value->c_lflag &= ~(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
    value->c_cflag = (value->c_cflag & ~(CSIZE | PARENB)) | CS8;
    value->c_cc[VMIN] = 1;
    value->c_cc[VTIME] = 0;
}
