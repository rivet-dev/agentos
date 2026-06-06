#include <unistd.h>
#ifdef getentropy
#undef getentropy
#endif
int (*foo)(void *, size_t) = getentropy;
int main(void) { return 0; }
