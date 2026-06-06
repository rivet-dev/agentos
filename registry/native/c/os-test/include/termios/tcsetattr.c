#include <termios.h>
#ifdef tcsetattr
#undef tcsetattr
#endif
int (*foo)(int, int, const struct termios *) = tcsetattr;
int main(void) { return 0; }
