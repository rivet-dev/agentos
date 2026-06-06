#include <termios.h>
#ifdef cfsetospeed
#undef cfsetospeed
#endif
int (*foo)(struct termios *, speed_t) = cfsetospeed;
int main(void) { return 0; }
