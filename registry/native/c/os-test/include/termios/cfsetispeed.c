#include <termios.h>
#ifdef cfsetispeed
#undef cfsetispeed
#endif
int (*foo)(struct termios *, speed_t) = cfsetispeed;
int main(void) { return 0; }
