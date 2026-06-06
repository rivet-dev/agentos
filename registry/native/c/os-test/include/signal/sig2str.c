#include <signal.h>
#ifdef sig2str
#undef sig2str
#endif
int (*foo)(int, char *) = sig2str;
int main(void) { return 0; }
