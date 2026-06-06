#include <termios.h>
#ifdef tcdrain
#undef tcdrain
#endif
int (*foo)(int) = tcdrain;
int main(void) { return 0; }
