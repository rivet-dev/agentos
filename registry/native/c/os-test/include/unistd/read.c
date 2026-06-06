#include <unistd.h>
#ifdef read
#undef read
#endif
ssize_t (*foo)(int, void *, size_t) = read;
int main(void) { return 0; }
