#include <unistd.h>
#ifdef confstr
#undef confstr
#endif
size_t (*foo)(int, char *, size_t) = confstr;
int main(void) { return 0; }
