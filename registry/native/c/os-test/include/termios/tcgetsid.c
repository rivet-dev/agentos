#include <termios.h>
#ifdef tcgetsid
#undef tcgetsid
#endif
pid_t (*foo)(int) = tcgetsid;
int main(void) { return 0; }
