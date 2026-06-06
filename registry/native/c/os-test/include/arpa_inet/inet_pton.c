#include <arpa/inet.h>
#ifdef inet_pton
#undef inet_pton
#endif
int (*foo)(int, const char *restrict, void *restrict) = inet_pton;
int main(void) { return 0; }
