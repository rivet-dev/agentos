#include <sys/socket.h>
#ifdef socketpair
#undef socketpair
#endif
int (*foo)(int, int, int, int [2]) = socketpair;
int main(void) { return 0; }
