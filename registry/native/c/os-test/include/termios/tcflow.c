#include <termios.h>
#ifdef tcflow
#undef tcflow
#endif
int (*foo)(int, int) = tcflow;
int main(void) { return 0; }
