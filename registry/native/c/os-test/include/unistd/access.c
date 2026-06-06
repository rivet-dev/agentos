#include <unistd.h>
#ifdef access
#undef access
#endif
int (*foo)(const char *, int) = access;
int main(void) { return 0; }
