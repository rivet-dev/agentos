#include <termios.h>
#ifdef tcgetwinsize
#undef tcgetwinsize
#endif
int (*foo)(int, struct winsize *) = tcgetwinsize;
int main(void) { return 0; }
