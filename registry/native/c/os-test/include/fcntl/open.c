#include <fcntl.h>
#ifdef open
#undef open
#endif
int (*foo)(const char *, int, ...) = open;
int main(void) { return 0; }
