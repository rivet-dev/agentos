#include <unistd.h>
#ifdef linkat
#undef linkat
#endif
int (*foo)(int, const char *, int, const char *, int) = linkat;
int main(void) { return 0; }
