#include <unistd.h>
#ifdef ttyname_r
#undef ttyname_r
#endif
int (*foo)(int, char *, size_t) = ttyname_r;
int main(void) { return 0; }
