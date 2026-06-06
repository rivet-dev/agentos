#include <termios.h>
#ifdef cfgetospeed
#undef cfgetospeed
#endif
speed_t (*foo)(const struct termios *) = cfgetospeed;
int main(void) { return 0; }
