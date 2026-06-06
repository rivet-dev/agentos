#include <unistd.h>
#ifdef unlink
#undef unlink
#endif
int (*foo)(const char *) = unlink;
int main(void) { return 0; }
