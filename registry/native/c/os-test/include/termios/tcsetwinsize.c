#include <termios.h>
#ifdef tcsetwinsize
#undef tcsetwinsize
#endif
int (*foo)(int, const struct winsize *) = tcsetwinsize;
int main(void) { return 0; }
