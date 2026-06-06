#include <sys/socket.h>
#ifdef sockatmark
#undef sockatmark
#endif
int (*foo)(int) = sockatmark;
int main(void) { return 0; }
