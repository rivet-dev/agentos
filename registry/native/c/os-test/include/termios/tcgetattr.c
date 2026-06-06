#include <termios.h>
#ifdef tcgetattr
#undef tcgetattr
#endif
int (*foo)(int, struct termios *) = tcgetattr;
int main(void) { return 0; }
