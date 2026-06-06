#include <wchar.h>
#ifdef wcsspn
#undef wcsspn
#endif
size_t (*foo)(const wchar_t *, const wchar_t *) = wcsspn;
int main(void) { return 0; }
