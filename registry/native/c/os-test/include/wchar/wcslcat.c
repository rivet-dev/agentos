#include <wchar.h>
#ifdef wcslcat
#undef wcslcat
#endif
size_t (*foo)(wchar_t *restrict, const wchar_t *restrict, size_t) = wcslcat;
int main(void) { return 0; }
