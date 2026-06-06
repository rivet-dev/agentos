#include <fcntl.h>
#ifdef fcntl
#undef fcntl
#endif
int (*foo)(int, int, ...) = fcntl;
int main(void) { return 0; }
