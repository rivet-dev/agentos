#include <termios.h>
#ifdef tcflush
#undef tcflush
#endif
int (*foo)(int, int) = tcflush;
int main(void) { return 0; }
