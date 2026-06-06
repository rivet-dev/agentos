#include <termios.h>
#ifdef tcsendbreak
#undef tcsendbreak
#endif
int (*foo)(int, int) = tcsendbreak;
int main(void) { return 0; }
