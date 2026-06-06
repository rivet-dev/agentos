#include <sys/socket.h>
#ifdef listen
#undef listen
#endif
int (*foo)(int, int) = listen;
int main(void) { return 0; }
