#include <termios.h>
#ifdef cfgetispeed
#undef cfgetispeed
#endif
speed_t (*foo)(const struct termios *) = cfgetispeed;
int main(void) { return 0; }
